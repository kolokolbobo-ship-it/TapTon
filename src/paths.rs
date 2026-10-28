//! Locations of all per-user data. Nothing is ever written to the current directory.

use directories::ProjectDirs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub settings_file: PathBuf,
    pub library_file: PathBuf,
    /// TapTone-managed copies of imported and downloaded sounds.
    pub sounds_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root: PathBuf = root.into();
        Self {
            settings_file: root.join("settings.json"),
            library_file: root.join("library.json"),
            sounds_dir: root.join("sounds"),
            logs_dir: root.join("logs"),
            root,
        }
    }

    /// Standard per-user location (on Windows: `%LOCALAPPDATA%\TapTone\TapTone\data`).
    pub fn discover() -> Option<Self> {
        ProjectDirs::from("app", "TapTone", "TapTone").map(|d| Self::from_root(d.data_local_dir()))
    }

    pub fn ensure_dirs(&self) -> io::Result<()> {
        for dir in [&self.root, &self.sounds_dir, &self.logs_dir] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
