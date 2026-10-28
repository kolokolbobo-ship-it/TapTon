//! User settings, persisted as JSON. Every field has a default so older or partial files still load.

use crate::errors::StorageError;
use crate::fsutil::{backup_corrupt, load_json, save_json, LoadStatus};
use crate::library::normalize_hotkey;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What happens when a sound that is already playing is triggered again.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RetriggerPolicy {
    /// Stop the running instance and start over.
    #[default]
    Restart,
    /// Ignore the new trigger while the sound is still playing.
    Ignore,
    /// Start an additional overlapping instance.
    Overlap,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CloseBehavior {
    /// Closing the window quits TapTone (and releases all hotkeys).
    #[default]
    Exit,
    /// Closing the window minimizes it so hotkeys keep working.
    Minimize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Headphones / speakers. Device names as reported by the audio host; `None` means the system default.
    pub main_output: Option<String>,
    /// The microphone route: the output device (normally a virtual audio cable) whose recording
    /// side is used as the microphone in Discord, games, etc. `None` = nothing selected.
    /// (The field names are kept from older versions so existing settings files still load.)
    pub secondary_output: Option<String>,
    /// Whether sounds are also sent to the microphone route.
    pub secondary_enabled: bool,
    pub master_volume: f32,
    /// Volume of the headphones route (0..=1), on top of the master volume.
    pub main_volume: f32,
    /// Volume of the microphone route (0..=1), on top of the master volume.
    pub mic_volume: f32,
    /// Quick mute of the microphone route; normal playback continues.
    pub mic_muted: bool,
    /// Whether the user's real microphone is mixed into the microphone route, so that a voice chat
    /// hears voice and sounds through one device. Off by default: TapTone does not open a
    /// microphone unless asked to.
    pub voice_enabled: bool,
    /// The real microphone to mix in (a recording device, never an output). `None` = the system
    /// default microphone.
    pub voice_input: Option<String>,
    /// Volume of the user's voice in the microphone route (0..=1). Independent of the sound volumes.
    pub voice_volume: f32,
    /// Global hotkey that toggles `mic_muted`, in canonical form (e.g. `Ctrl+Alt+M`).
    pub mic_mute_hotkey: Option<String>,
    /// Gain applied to every newly started sound.
    pub default_sound_volume: f32,
    /// If false, starting a sound stops all others first.
    pub allow_overlap: bool,
    pub retrigger: RetriggerPolicy,
    /// UI language code. Only "en" ships strings in Part 1.
    pub language: String,
    pub close_behavior: CloseBehavior,
    /// Copy imported files into the TapTone sounds folder.
    pub copy_on_import: bool,
    /// When removing a library entry, also delete the TapTone-managed copy (never the user's original).
    pub delete_managed_copy_on_remove: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            main_output: None,
            secondary_output: None,
            secondary_enabled: false,
            master_volume: 1.0,
            main_volume: 1.0,
            mic_volume: 1.0,
            mic_muted: false,
            voice_enabled: false,
            voice_input: None,
            voice_volume: 1.0,
            mic_mute_hotkey: None,
            default_sound_volume: 1.0,
            allow_overlap: true,
            retrigger: RetriggerPolicy::Restart,
            language: "en".to_string(),
            close_behavior: CloseBehavior::Exit,
            copy_on_import: true,
            delete_managed_copy_on_remove: false,
        }
    }
}

fn clamp_unit(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

fn clean_device(name: Option<String>) -> Option<String> {
    name.filter(|n| !n.trim().is_empty())
}

impl Settings {
    /// Build settings from a parsed JSON object, field by field: a field with a wrong type or an
    /// unknown name is ignored (defaults are kept) instead of resetting the whole file.
    fn from_object(user: &serde_json::Map<String, serde_json::Value>) -> Settings {
        use serde_json::Value;
        let defaults = match serde_json::to_value(Settings::default()) {
            Ok(Value::Object(m)) => m,
            _ => return Settings::default(),
        };
        let mut merged = defaults.clone();
        for (key, value) in user {
            if !defaults.contains_key(key) {
                continue;
            }
            let mut trial = merged.clone();
            trial.insert(key.clone(), value.clone());
            if serde_json::from_value::<Settings>(Value::Object(trial.clone())).is_ok() {
                merged = trial;
            } else {
                tracing::warn!("settings: ignoring invalid value for \"{key}\"");
            }
        }
        serde_json::from_value(Value::Object(merged)).unwrap_or_default()
    }

    /// Bring hand-edited or corrupt values back into valid ranges.
    pub fn sanitize(&mut self) {
        self.master_volume = clamp_unit(self.master_volume, 1.0);
        self.main_volume = clamp_unit(self.main_volume, 1.0);
        self.mic_volume = clamp_unit(self.mic_volume, 1.0);
        self.voice_volume = clamp_unit(self.voice_volume, 1.0);
        self.voice_input = clean_device(self.voice_input.take());
        self.main_output = clean_device(self.main_output.take());
        self.secondary_output = clean_device(self.secondary_output.take());
        self.mic_mute_hotkey = self.mic_mute_hotkey.take().and_then(|h| normalize_hotkey(&h).ok());
        self.default_sound_volume = clamp_unit(self.default_sound_volume, 1.0);
        if self.language.trim().is_empty() {
            self.language = "en".to_string();
        }
    }

    pub fn load(path: &Path) -> Result<(Settings, LoadStatus), StorageError> {
        let (value, status): (serde_json::Value, LoadStatus) = load_json(path)?;
        let (mut s, status) = match (value, status) {
            (serde_json::Value::Object(map), status) => (Settings::from_object(&map), status),
            (serde_json::Value::Null, status) => (Settings::default(), status),
            (_, _) => {
                // Valid JSON, but not an object: treat the file as damaged.
                let backup = backup_corrupt(path);
                (
                    Settings::default(),
                    LoadStatus::Recovered {
                        backup,
                        reason: "the file does not contain a settings object".to_string(),
                    },
                )
            }
        };
        s.sanitize();
        Ok((s, status))
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        save_json(path, self)
    }
}
