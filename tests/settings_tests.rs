use std::fs;
use taptone::fsutil::LoadStatus;
use taptone::settings::{CloseBehavior, RetriggerPolicy, Settings};

#[test]
fn missing_file_gives_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let (s, status) = Settings::load(&dir.path().join("settings.json")).unwrap();
    assert_eq!(status, LoadStatus::Missing);
    assert_eq!(s, Settings::default());
}

#[test]
fn roundtrip_preserves_values_and_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let s = Settings {
        main_output: Some("Speakers".into()),
        secondary_output: Some("CABLE Input".into()),
        secondary_enabled: true,
        master_volume: 0.5,
        retrigger: RetriggerPolicy::Overlap,
        close_behavior: CloseBehavior::Minimize,
        ..Settings::default()
    };
    s.save(&path).unwrap();
    let (loaded, status) = Settings::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert_eq!(loaded, s);
    assert!(!dir.path().join("settings.json.tmp").exists());
}

#[test]
fn corrupt_json_is_backed_up_and_defaults_used() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, b"{ this is not json").unwrap();
    let (s, status) = Settings::load(&path).unwrap();
    assert_eq!(s, Settings::default());
    match status {
        LoadStatus::Recovered { backup: Some(b), .. } => {
            assert!(b.exists());
            assert_eq!(fs::read(&b).unwrap(), b"{ this is not json");
        }
        other => panic!("unexpected status {other:?}"),
    }
    assert!(!path.exists());
}

#[test]
fn partial_file_and_unknown_fields_are_tolerated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, br#"{"secondary_enabled": true, "future_option": 42}"#).unwrap();
    let (s, status) = Settings::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert!(s.secondary_enabled);
    assert_eq!(s.master_volume, 1.0);
}

#[test]
fn out_of_range_volumes_are_sanitized() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(
        &path,
        br#"{"master_volume": 7.5, "default_sound_volume": -3.0, "language": " "}"#,
    )
    .unwrap();
    let (s, _) = Settings::load(&path).unwrap();
    assert_eq!(s.master_volume, 1.0);
    assert_eq!(s.default_sound_volume, 0.0);
    assert_eq!(s.language, "en");
}

#[test]
fn old_settings_file_gets_safe_defaults_for_new_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(
        &path,
        br#"{"main_output":"Speakers","secondary_output":"CABLE Input (VB-Audio Virtual Cable)","secondary_enabled":true,"master_volume":0.4}"#,
    )
    .unwrap();
    let (s, status) = Settings::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert_eq!(s.main_output.as_deref(), Some("Speakers"));
    assert_eq!(
        s.secondary_output.as_deref(),
        Some("CABLE Input (VB-Audio Virtual Cable)")
    );
    assert!(s.secondary_enabled);
    assert_eq!(s.master_volume, 0.4);
    assert_eq!((s.main_volume, s.mic_volume, s.mic_muted), (1.0, 1.0, false));
    assert_eq!(s.mic_mute_hotkey, None);
}

#[test]
fn one_invalid_field_does_not_reset_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(
        &path,
        br#"{"master_volume":"loud","mic_volume":0.25,"main_output":"Speakers","secondary_enabled":"yes"}"#,
    )
    .unwrap();
    let (s, status) = Settings::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert_eq!(s.master_volume, 1.0);
    assert!(!s.secondary_enabled);
    assert_eq!(s.mic_volume, 0.25);
    assert_eq!(s.main_output.as_deref(), Some("Speakers"));
    assert!(path.exists());
}

#[test]
fn non_object_json_is_treated_as_damaged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, b"[1,2,3]").unwrap();
    let (s, status) = Settings::load(&path).unwrap();
    assert_eq!(s, Settings::default());
    assert!(matches!(status, LoadStatus::Recovered { .. }));
}

#[test]
fn new_values_are_sanitized_and_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(
        &path,
        br#"{"main_volume":3.0,"mic_volume":-1.0,"mic_mute_hotkey":"banana","main_output":"  "}"#,
    )
    .unwrap();
    let (s, _) = Settings::load(&path).unwrap();
    assert_eq!((s.main_volume, s.mic_volume), (1.0, 0.0));
    assert_eq!(s.mic_mute_hotkey, None);
    assert_eq!(s.main_output, None);

    let good = Settings {
        mic_mute_hotkey: Some("ctrl+alt+m".into()),
        mic_muted: true,
        mic_volume: 0.6,
        ..Settings::default()
    };
    let mut expected = good.clone();
    expected.sanitize();
    expected.save(&path).unwrap();
    let (loaded, _) = Settings::load(&path).unwrap();
    assert_eq!(loaded.mic_mute_hotkey.as_deref(), Some("Ctrl+Alt+M"));
    assert!(loaded.mic_muted);
    assert_eq!(loaded.mic_volume, 0.6);
}

#[test]
fn voice_settings_default_to_off_and_survive_a_roundtrip() {
    let d = Settings::default();
    assert!(
        !d.voice_enabled,
        "the microphone must never be opened unless the user asks for it"
    );
    assert_eq!(d.voice_input, None);
    assert_eq!(d.voice_volume, 1.0);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let s = Settings {
        voice_enabled: true,
        voice_input: Some("Microphone (USB Headset)".into()),
        voice_volume: 0.4,
        ..Settings::default()
    };
    s.save(&path).unwrap();
    let (loaded, status) = Settings::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert_eq!(loaded, s);
}

#[test]
fn settings_files_from_older_versions_load_with_the_microphone_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(
        &path,
        br#"{"secondary_enabled": true, "secondary_output": "CABLE Input", "mic_volume": 0.3}"#,
    )
    .unwrap();
    let (s, status) = Settings::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert!(s.secondary_enabled);
    assert_eq!(s.secondary_output.as_deref(), Some("CABLE Input"));
    assert!(!s.voice_enabled);
    assert_eq!(s.voice_input, None);
}

#[test]
fn capture_apo_values_in_old_settings_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    // settings.json written by an APO-era build: the fields are dropped, the cable route stays off.
    fs::write(
        &path,
        br#"{"secondary_enabled": true, "capture_apo_enabled": true, "capture_apo_virtual_mic": true, "capture_apo_target": "Mic"}"#,
    )
    .unwrap();
    let (loaded, status) = Settings::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert!(loaded.secondary_enabled);
    assert!(loaded.secondary_output.is_none());
    assert!(!loaded.voice_enabled);
}

#[test]
fn bad_voice_values_are_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(
        &path,
        br#"{"voice_volume": 9.0, "voice_input": "   ", "voice_enabled": "yes"}"#,
    )
    .unwrap();
    let (s, _) = Settings::load(&path).unwrap();
    assert_eq!(s.voice_volume, 1.0);
    assert_eq!(s.voice_input, None);
    assert!(!s.voice_enabled, "a value of the wrong type is ignored, not guessed");
}
