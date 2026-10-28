//! Small filesystem helpers: atomic writes and tolerant JSON loading.

use crate::errors::StorageError;
use serde::{de::DeserializeOwned, Serialize};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadStatus {
    /// The file existed and parsed correctly.
    Loaded,
    /// The file did not exist; defaults were used.
    Missing,
    /// The file was corrupt; defaults were used and the bad file was moved to `backup` if possible.
    Recovered { backup: Option<PathBuf>, reason: String },
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn io_err(path: &Path, source: io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Write `bytes` to a temporary sibling file, flush it to disk, then rename over `path`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    let mut name: OsString = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| OsString::from("data"));
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    {
        let mut f = File::create(&tmp).map_err(|e| io_err(&tmp, e))?;
        f.write_all(bytes).map_err(|e| io_err(&tmp, e))?;
        f.sync_all().map_err(|e| io_err(&tmp, e))?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        if let Err(rm) = fs::remove_file(&tmp) {
            tracing::debug!("could not remove temp file {}: {rm}", tmp.display());
        }
        return Err(io_err(path, e));
    }
    Ok(())
}

/// Move a corrupt file aside so the user can inspect it. Returns the backup path on success.
pub fn backup_corrupt(path: &Path) -> Option<PathBuf> {
    let mut name: OsString = path.file_name()?.to_os_string();
    name.push(format!(".corrupt-{}", unix_now()));
    let backup = path.with_file_name(name);
    match fs::rename(path, &backup) {
        Ok(()) => Some(backup),
        Err(e) => {
            tracing::warn!("could not back up corrupt file {}: {e}", path.display());
            None
        }
    }
}

pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<(T, LoadStatus), StorageError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((T::default(), LoadStatus::Missing)),
        Err(e) => return Err(io_err(path, e)),
    };
    match serde_json::from_slice::<T>(&bytes) {
        Ok(v) => Ok((v, LoadStatus::Loaded)),
        Err(e) => {
            tracing::warn!("corrupt JSON in {}: {e}", path.display());
            let backup = backup_corrupt(path);
            Ok((
                T::default(),
                LoadStatus::Recovered {
                    backup,
                    reason: e.to_string(),
                },
            ))
        }
    }
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| StorageError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    write_atomic(path, &bytes)
}
