//! Global (system-wide) hotkeys.
//!
//! * `parse_hotkey` converts TapTone's canonical strings (`Ctrl+Alt+K`, see
//!   `library::normalize_hotkey`) into `global_hotkey` keys.
//! * `Hotkeys` owns the OS registrations. It must be created and used on the thread that runs the
//!   window event loop (on Windows the hotkey window lives on that thread's message pump).
//! * A listener thread receives key presses and calls a callback with the sound id, so hotkeys keep
//!   working even when the window is minimized and not repainting.
//! * Everything is released again on `unregister_all` / `Drop`, i.e. when TapTone exits.

use crate::library::normalize_hotkey;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Repeats of a held key arriving faster than this are ignored.
const DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error("{0}")]
    Invalid(String),
    #[error("This key combination is already used by another program or cannot be registered ({0})")]
    Os(String),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

const LETTERS: [Code; 26] = [
    Code::KeyA,
    Code::KeyB,
    Code::KeyC,
    Code::KeyD,
    Code::KeyE,
    Code::KeyF,
    Code::KeyG,
    Code::KeyH,
    Code::KeyI,
    Code::KeyJ,
    Code::KeyK,
    Code::KeyL,
    Code::KeyM,
    Code::KeyN,
    Code::KeyO,
    Code::KeyP,
    Code::KeyQ,
    Code::KeyR,
    Code::KeyS,
    Code::KeyT,
    Code::KeyU,
    Code::KeyV,
    Code::KeyW,
    Code::KeyX,
    Code::KeyY,
    Code::KeyZ,
];

const DIGITS: [Code; 10] = [
    Code::Digit0,
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
    Code::Digit9,
];

const NUMPAD: [Code; 10] = [
    Code::Numpad0,
    Code::Numpad1,
    Code::Numpad2,
    Code::Numpad3,
    Code::Numpad4,
    Code::Numpad5,
    Code::Numpad6,
    Code::Numpad7,
    Code::Numpad8,
    Code::Numpad9,
];

const FKEYS: [Code; 24] = [
    Code::F1,
    Code::F2,
    Code::F3,
    Code::F4,
    Code::F5,
    Code::F6,
    Code::F7,
    Code::F8,
    Code::F9,
    Code::F10,
    Code::F11,
    Code::F12,
    Code::F13,
    Code::F14,
    Code::F15,
    Code::F16,
    Code::F17,
    Code::F18,
    Code::F19,
    Code::F20,
    Code::F21,
    Code::F22,
    Code::F23,
    Code::F24,
];

fn code_from_name(name: &str) -> Option<Code> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() {
            return Some(LETTERS[(c as u8 - b'A') as usize]);
        }
        if c.is_ascii_digit() {
            return Some(DIGITS[(c as u8 - b'0') as usize]);
        }
        return None;
    }
    if let Some(rest) = name.strip_prefix("Num") {
        if rest.len() == 1 {
            if let Some(d) = rest.chars().next().and_then(|c| c.to_digit(10)) {
                return Some(NUMPAD[d as usize]);
            }
        }
    }
    if let Some(rest) = name.strip_prefix('F') {
        if let Ok(n) = rest.parse::<usize>() {
            if (1..=24).contains(&n) {
                return Some(FKEYS[n - 1]);
            }
        }
    }
    Some(match name {
        "Space" => Code::Space,
        "Escape" => Code::Escape,
        "Enter" => Code::Enter,
        "Tab" => Code::Tab,
        "Backspace" => Code::Backspace,
        "Delete" => Code::Delete,
        "Insert" => Code::Insert,
        "Home" => Code::Home,
        "End" => Code::End,
        "PageUp" => Code::PageUp,
        "PageDown" => Code::PageDown,
        "Up" => Code::ArrowUp,
        "Down" => Code::ArrowDown,
        "Left" => Code::ArrowLeft,
        "Right" => Code::ArrowRight,
        _ => return None,
    })
}

/// Convert a hotkey string such as `ctrl+alt+k` into a `global_hotkey` key.
pub fn parse_hotkey(input: &str) -> Result<HotKey, HotkeyError> {
    let normalized = normalize_hotkey(input).map_err(|e| HotkeyError::Invalid(e.to_string()))?;
    let mut mods = Modifiers::empty();
    let mut code = None;
    for token in normalized.split('+') {
        match token {
            "Ctrl" => mods |= Modifiers::CONTROL,
            "Alt" => mods |= Modifiers::ALT,
            "Shift" => mods |= Modifiers::SHIFT,
            "Win" => mods |= Modifiers::SUPER,
            key => code = code_from_name(key),
        }
    }
    let code = code.ok_or_else(|| HotkeyError::Invalid(format!("Unsupported key in \"{input}\"")))?;
    let mods = if mods.is_empty() { None } else { Some(mods) };
    Ok(HotKey::new(mods, code))
}

/// Owner of all OS-level hotkey registrations.
pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    by_sound: HashMap<String, HotKey>,
    /// Hotkey id -> sound id, shared with the listener thread.
    ids: Arc<Mutex<HashMap<u32, String>>>,
    stop: Arc<AtomicBool>,
    listener_started: bool,
}

impl Hotkeys {
    /// Must be called on the thread that runs the window event loop.
    pub fn new() -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new().map_err(|e| format!("Global hotkeys are unavailable: {e}"))?;
        Ok(Self {
            manager,
            by_sound: HashMap::new(),
            ids: Arc::new(Mutex::new(HashMap::new())),
            stop: Arc::new(AtomicBool::new(false)),
            listener_started: false,
        })
    }

    /// Start the background thread that reports key presses. `on_trigger` receives the sound id.
    /// Calling this more than once has no effect.
    pub fn start_listener<F>(&mut self, on_trigger: F)
    where
        F: Fn(&str) + Send + 'static,
    {
        if self.listener_started {
            return;
        }
        self.listener_started = true;
        let ids = Arc::clone(&self.ids);
        let stop = Arc::clone(&self.stop);
        let spawned = std::thread::Builder::new()
            .name("taptone-hotkeys".to_string())
            .spawn(move || {
                let rx = GlobalHotKeyEvent::receiver();
                let mut last: HashMap<u32, Instant> = HashMap::new();
                loop {
                    match rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(event) => {
                            if event.state != HotKeyState::Pressed {
                                continue;
                            }
                            let now = Instant::now();
                            let repeat = last.get(&event.id).is_some_and(|t| now.duration_since(*t) < DEBOUNCE);
                            last.insert(event.id, now);
                            if repeat {
                                continue;
                            }
                            let sound = lock(&ids).get(&event.id).cloned();
                            if let Some(sound_id) = sound {
                                on_trigger(&sound_id);
                            }
                        }
                        Err(_) => {
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::error!("could not start the hotkey listener: {e}");
        }
    }

    /// Register `hotkey` for a sound, replacing the sound's previous hotkey. If the new hotkey cannot
    /// be registered, the old one stays active.
    pub fn register(&mut self, sound_id: &str, hotkey: &str) -> Result<(), HotkeyError> {
        let hk = parse_hotkey(hotkey)?;
        if self.by_sound.get(sound_id).is_some_and(|old| old.id() == hk.id()) {
            return Ok(());
        }
        let new_id = hk.id();
        self.manager.register(hk).map_err(|e| HotkeyError::Os(e.to_string()))?;
        if let Some(old) = self.by_sound.insert(sound_id.to_string(), hk) {
            let old_id = old.id();
            if let Err(e) = self.manager.unregister(old) {
                tracing::debug!("could not release the old hotkey: {e}");
            }
            lock(&self.ids).remove(&old_id);
        }
        lock(&self.ids).insert(new_id, sound_id.to_string());
        Ok(())
    }

    /// Release the hotkey of a sound (no-op if it has none).
    pub fn unregister(&mut self, sound_id: &str) {
        if let Some(hk) = self.by_sound.remove(sound_id) {
            let id = hk.id();
            if let Err(e) = self.manager.unregister(hk) {
                tracing::debug!("could not release a hotkey: {e}");
            }
            lock(&self.ids).remove(&id);
        }
    }

    /// Release every hotkey. Called on exit so no registration outlives the application.
    pub fn unregister_all(&mut self) {
        for (_, hk) in self.by_sound.drain() {
            if let Err(e) = self.manager.unregister(hk) {
                tracing::debug!("could not release a hotkey: {e}");
            }
        }
        lock(&self.ids).clear();
    }

    /// Make the registrations match `wanted` (`(sound_id, hotkey)` pairs). Returns the pairs that
    /// could not be registered together with a user-readable reason.
    pub fn sync(&mut self, wanted: &[(String, String)]) -> Vec<(String, String)> {
        let keep: Vec<&str> = wanted.iter().map(|(id, _)| id.as_str()).collect();
        let stale: Vec<String> = self
            .by_sound
            .keys()
            .filter(|id| !keep.contains(&id.as_str()))
            .cloned()
            .collect();
        for id in stale {
            self.unregister(&id);
        }
        let mut failures = Vec::new();
        for (id, hotkey) in wanted {
            if let Err(e) = self.register(id, hotkey) {
                failures.push((id.clone(), e.to_string()));
            }
        }
        failures
    }

    /// Number of active registrations.
    pub fn active_count(&self) -> usize {
        self.by_sound.len()
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.unregister_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_letter() {
        let hk = parse_hotkey("ctrl+alt+k").unwrap();
        assert_eq!(hk, HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyK));
    }

    #[test]
    fn parses_bare_function_key() {
        let hk = parse_hotkey("F5").unwrap();
        assert_eq!(hk, HotKey::new(None, Code::F5));
    }

    #[test]
    fn parses_numpad_and_digits() {
        assert_eq!(
            parse_hotkey("Ctrl+Num3").unwrap(),
            HotKey::new(Some(Modifiers::CONTROL), Code::Numpad3)
        );
        assert_eq!(
            parse_hotkey("Ctrl+7").unwrap(),
            HotKey::new(Some(Modifiers::CONTROL), Code::Digit7)
        );
    }

    #[test]
    fn rejects_bare_letter_and_garbage() {
        assert!(parse_hotkey("K").is_err());
        assert!(parse_hotkey("Ctrl+").is_err());
        assert!(parse_hotkey("Ctrl+Banana").is_err());
    }
}
