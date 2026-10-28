//! Sound library: entries, import, search, favorites, hotkey assignments and persistence.
//! Removing an entry never deletes the user's original file.

use crate::errors::{LibraryError, StorageError};
use crate::fsutil::{load_json, save_json, unix_now, write_atomic, LoadStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
}

impl AudioFormat {
    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Wav => "wav",
            AudioFormat::Mp3 => "mp3",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "wav" => Some(AudioFormat::Wav),
            "mp3" => Some(AudioFormat::Mp3),
            _ => None,
        }
    }

    /// Cheap content check on the first bytes. The decoder (Part 2) performs the full probe.
    pub fn sniff(head: &[u8]) -> Option<Self> {
        if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
            return Some(AudioFormat::Wav);
        }
        if head.len() >= 3 && &head[0..3] == b"ID3" {
            return Some(AudioFormat::Mp3);
        }
        // MPEG audio frame sync: 11 set bits, valid version and layer.
        if head.len() >= 2
            && head[0] == 0xFF
            && head[1] & 0xE0 == 0xE0
            && head[1] & 0x18 != 0x08
            && head[1] & 0x06 != 0x00
        {
            return Some(AudioFormat::Mp3);
        }
        None
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub enum ImportSource {
    #[default]
    Local,
    /// Download URL with query string and credentials removed.
    Url(String),
}

/// Highest per-sound volume (a sound can be boosted to twice its original level).
pub const MAX_ENTRY_VOLUME: f32 = 2.0;

fn default_volume() -> f32 {
    1.0
}

fn clamp_volume(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, MAX_ENTRY_VOLUME)
    } else {
        1.0
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SoundEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub duration_secs: Option<f64>,
    #[serde(default)]
    pub source: ImportSource,
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default)]
    pub favorite: bool,
    /// Unix timestamp (seconds) of when the entry was added.
    #[serde(default)]
    pub added_at: u64,
    /// Volume of this sound (0.0..=2.0), multiplied with the default sound volume.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Fingerprint of the file content, used to detect duplicates.
    #[serde(default)]
    pub content_hash: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Library {
    pub version: u32,
    pub entries: Vec<SoundEntry>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
}

fn io_error(path: &Path, source: io::Error) -> LibraryError {
    if source.kind() == io::ErrorKind::NotFound {
        LibraryError::NotFound(path.to_path_buf())
    } else {
        LibraryError::Storage(StorageError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

impl Library {
    /// Load the library entry by entry: a damaged entry is skipped (and kept in a side file)
    /// instead of making the whole library unreadable.
    pub fn load(path: &Path) -> Result<(Library, LoadStatus), StorageError> {
        use serde_json::Value;
        let (value, status): (Value, LoadStatus) = load_json(path)?;
        let map = match value {
            Value::Object(map) => map,
            Value::Null => return Ok((Library::default(), status)),
            _ => {
                let backup = crate::fsutil::backup_corrupt(path);
                let reason = "the file does not contain a library object".to_string();
                return Ok((Library::default(), LoadStatus::Recovered { backup, reason }));
            }
        };
        let version = map.get("version").and_then(Value::as_u64).map_or(1, |v| v as u32);
        let raw: Vec<Value> = match map.get("entries") {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        let mut entries: Vec<SoundEntry> = Vec::with_capacity(raw.len());
        let mut seen: HashSet<String> = HashSet::new();
        let mut skipped: Vec<Value> = Vec::new();
        for item in raw {
            match serde_json::from_value::<SoundEntry>(item.clone()) {
                Ok(mut e) if !e.id.is_empty() && seen.insert(e.id.clone()) => {
                    e.volume = clamp_volume(e.volume);
                    entries.push(e);
                }
                _ => skipped.push(item),
            }
        }
        let lib = Library { version, entries };
        if skipped.is_empty() {
            return Ok((lib, status));
        }
        let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(format!(".skipped-{}", unix_now()));
        let side = path.with_file_name(name);
        let backup = match serde_json::to_vec_pretty(&skipped) {
            Ok(bytes) => write_atomic(&side, &bytes).ok().map(|_| side),
            Err(_) => None,
        };
        let reason = format!("{} damaged or duplicate entries were skipped", skipped.len());
        Ok((lib, LoadStatus::Recovered { backup, reason }))
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        save_json(path, self)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn add_entry(&mut self, entry: SoundEntry) -> String {
        let id = entry.id.clone();
        self.entries.push(entry);
        id
    }

    pub fn get(&self, id: &str) -> Option<&SoundEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SoundEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Remove the entry from the library. The audio file on disk is left untouched.
    pub fn remove(&mut self, id: &str) -> Result<SoundEntry, LibraryError> {
        let idx = self
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or_else(|| LibraryError::UnknownSound(id.to_string()))?;
        Ok(self.entries.remove(idx))
    }

    /// Case-insensitive substring search on the name. An empty query returns everything.
    pub fn search(&self, query: &str) -> Vec<&SoundEntry> {
        let q = query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|e| q.is_empty() || e.name.to_lowercase().contains(&q))
            .collect()
    }

    pub fn favorites(&self) -> Vec<&SoundEntry> {
        self.entries.iter().filter(|e| e.favorite).collect()
    }

    pub fn toggle_favorite(&mut self, id: &str) -> Result<bool, LibraryError> {
        let e = self
            .get_mut(id)
            .ok_or_else(|| LibraryError::UnknownSound(id.to_string()))?;
        e.favorite = !e.favorite;
        Ok(e.favorite)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), LibraryError> {
        let e = self
            .get_mut(id)
            .ok_or_else(|| LibraryError::UnknownSound(id.to_string()))?;
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            e.name = trimmed.to_string();
        }
        Ok(())
    }

    /// Set the volume of one sound (clamped to 0.0..=2.0).
    pub fn set_volume(&mut self, id: &str, volume: f32) -> Result<f32, LibraryError> {
        let e = self
            .get_mut(id)
            .ok_or_else(|| LibraryError::UnknownSound(id.to_string()))?;
        e.volume = clamp_volume(volume);
        Ok(e.volume)
    }

    /// An entry whose file content has the same fingerprint.
    pub fn find_by_hash(&self, hash: u64) -> Option<&SoundEntry> {
        self.entries.iter().find(|e| e.content_hash == Some(hash))
    }

    /// Ids of the entries whose file is missing, empty or not a regular file.
    pub fn missing_ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| check_available(e).is_err())
            .map(|e| e.id.clone())
            .collect()
    }

    pub fn set_duration(&mut self, id: &str, secs: f64) {
        if let Some(e) = self.get_mut(id) {
            e.duration_secs = Some(secs);
        }
    }

    /// The other entry that already uses `hotkey` (normalized form), if any.
    pub fn find_hotkey_conflict(&self, hotkey: &str, except_id: Option<&str>) -> Option<&SoundEntry> {
        self.entries
            .iter()
            .find(|e| Some(e.id.as_str()) != except_id && e.hotkey.as_deref() == Some(hotkey))
    }

    /// Assign (or clear, with `None`) the hotkey of a sound. Conflicts are rejected, never silent.
    pub fn set_hotkey(&mut self, id: &str, hotkey: Option<&str>) -> Result<Option<String>, LibraryError> {
        let normalized = match hotkey {
            Some(h) => Some(normalize_hotkey(h)?),
            None => None,
        };
        if let Some(h) = &normalized {
            if let Some(owner) = self.find_hotkey_conflict(h, Some(id)) {
                return Err(LibraryError::HotkeyConflict {
                    hotkey: h.clone(),
                    owner: owner.name.clone(),
                });
            }
        }
        let e = self
            .get_mut(id)
            .ok_or_else(|| LibraryError::UnknownSound(id.to_string()))?;
        e.hotkey = normalized.clone();
        Ok(normalized)
    }
}

/// Check that the file behind an entry still exists and is non-empty.
pub fn check_available(entry: &SoundEntry) -> Result<(), LibraryError> {
    let meta = fs::metadata(&entry.path).map_err(|e| io_error(&entry.path, e))?;
    if !meta.is_file() {
        return Err(LibraryError::NotFound(entry.path.clone()));
    }
    if meta.len() == 0 {
        return Err(LibraryError::EmptyFile(entry.path.clone()));
    }
    Ok(())
}

/// FNV-1a fingerprint of a file's content (not cryptographic; only used to spot duplicates).
pub fn file_fingerprint(path: &Path) -> io::Result<u64> {
    let mut f = File::open(path)?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Ok(hash)
}

/// Check that `src` is a non-empty WAV or MP3 file (by extension and by content).
pub fn validate_audio_file(src: &Path) -> Result<AudioFormat, LibraryError> {
    let meta = fs::metadata(src).map_err(|e| io_error(src, e))?;
    if !meta.is_file() {
        return Err(LibraryError::NotFound(src.to_path_buf()));
    }
    if meta.len() == 0 {
        return Err(LibraryError::EmptyFile(src.to_path_buf()));
    }
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
    AudioFormat::from_extension(ext).ok_or_else(|| LibraryError::UnsupportedFormat(format!(".{ext}")))?;
    let mut head = [0u8; 16];
    let n = File::open(src)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| io_error(src, e))?;
    AudioFormat::sniff(&head[..n])
        .ok_or_else(|| LibraryError::UnsupportedFormat("content is not WAV or MP3".to_string()))
}

/// Create a library entry for a local audio file. With `managed_dir`, the file is copied there
/// under a generated name; otherwise the entry points at the original path.
pub fn import_file(src: &Path, managed_dir: Option<&Path>, source: ImportSource) -> Result<SoundEntry, LibraryError> {
    let format = validate_audio_file(src)?;
    let content_hash = file_fingerprint(src).ok();

    let id = uuid::Uuid::new_v4().to_string();
    let path = match managed_dir {
        Some(dir) => {
            fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
            let dest = dir.join(format!("{id}.{}", format.extension()));
            fs::copy(src, &dest).map_err(|e| io_error(&dest, e))?;
            dest
        }
        None => src.to_path_buf(),
    };
    let name = src
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Sound")
        .to_string();

    Ok(SoundEntry {
        id,
        name,
        path,
        duration_secs: None,
        source,
        hotkey: None,
        favorite: false,
        added_at: unix_now(),
        volume: 1.0,
        content_hash,
    })
}

/// Delete the entry's file only if it is a TapTone-managed copy (lives directly in `managed_dir`).
/// Returns whether a file was deleted. User-owned files are never touched.
pub fn remove_managed_file(entry: &SoundEntry, managed_dir: &Path) -> Result<bool, LibraryError> {
    let (Ok(file), Ok(dir)) = (fs::canonicalize(&entry.path), fs::canonicalize(managed_dir)) else {
        return Ok(false);
    };
    if file.parent() != Some(dir.as_path()) {
        return Ok(false);
    }
    fs::remove_file(&file).map_err(|e| io_error(&file, e))?;
    Ok(true)
}

fn canonical_key(lower: &str) -> Option<String> {
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return c.is_ascii_alphanumeric().then(|| c.to_ascii_uppercase().to_string());
    }
    if let Some(rest) = lower.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Some(format!("F{n}"));
            }
        }
    }
    if let Some(rest) = lower.strip_prefix("num") {
        if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("Num{rest}"));
        }
    }
    let name = match lower {
        "space" => "Space",
        "esc" | "escape" => "Escape",
        "enter" | "return" => "Enter",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdn" => "PageDown",
        "up" | "arrowup" => "Up",
        "down" | "arrowdown" => "Down",
        "left" | "arrowleft" => "Left",
        "right" | "arrowright" => "Right",
        _ => return None,
    };
    Some(name.to_string())
}

/// Parse a hotkey such as `ctrl+alt+k` into the canonical form `Ctrl+Alt+K`.
/// Exactly one non-modifier key is required, plus at least one modifier unless it is an F-key
/// (a bare letter as a global hotkey would hijack normal typing).
pub fn normalize_hotkey(input: &str) -> Result<String, LibraryError> {
    let invalid = |msg: &str| LibraryError::InvalidHotkey(format!("\"{input}\": {msg}"));
    let (mut ctrl, mut alt, mut shift, mut win) = (false, false, false, false);
    let mut key: Option<String> = None;
    for raw in input.split('+') {
        let tok = raw.trim();
        if tok.is_empty() {
            return Err(invalid("empty key name"));
        }
        match tok.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            "win" | "super" | "meta" | "cmd" => win = true,
            other => {
                if key.is_some() {
                    return Err(invalid("only one non-modifier key is allowed"));
                }
                key = Some(canonical_key(other).ok_or_else(|| invalid("unknown key"))?);
            }
        }
    }
    let key = key.ok_or_else(|| invalid("no key given"))?;
    let is_fkey = key.len() > 1 && key.starts_with('F') && key[1..].chars().all(|c| c.is_ascii_digit());
    if !(ctrl || alt || shift || win) && !is_fkey {
        return Err(invalid("add a modifier such as Ctrl or Alt"));
    }
    let mut parts: Vec<&str> = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if win {
        parts.push("Win");
    }
    parts.push(&key);
    Ok(parts.join("+"))
}
