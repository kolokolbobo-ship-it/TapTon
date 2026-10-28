use std::fs;
use std::path::Path;
use taptone::errors::LibraryError;
use taptone::fsutil::LoadStatus;
use taptone::library::*;

fn write_fake_wav(path: &Path) {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&36u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&[0u8; 32]);
    fs::write(path, bytes).unwrap();
}

fn entry(name: &str) -> SoundEntry {
    SoundEntry {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        path: format!("/nonexistent/{name}.wav").into(),
        duration_secs: None,
        source: ImportSource::Local,
        hotkey: None,
        favorite: false,
        added_at: 0,
        volume: 1.0,
        content_hash: None,
    }
}

#[test]
fn empty_library_behaves() {
    let lib = Library::default();
    assert!(lib.is_empty());
    assert_eq!(lib.len(), 0);
    assert!(lib.search("anything").is_empty());
    assert!(lib.favorites().is_empty());
}

#[test]
fn missing_library_file_gives_empty_library() {
    let dir = tempfile::tempdir().unwrap();
    let (lib, status) = Library::load(&dir.path().join("library.json")).unwrap();
    assert!(lib.is_empty());
    assert_eq!(status, LoadStatus::Missing);
}

#[test]
fn search_is_case_insensitive_and_empty_query_returns_all() {
    let mut lib = Library::default();
    lib.add_entry(entry("Airhorn"));
    lib.add_entry(entry("Vine Boom"));
    assert_eq!(lib.search("AIR").len(), 1);
    assert_eq!(lib.search("").len(), 2);
    assert_eq!(lib.search("zzz").len(), 0);
}

#[test]
fn favorites_toggle() {
    let mut lib = Library::default();
    let id = lib.add_entry(entry("A"));
    assert!(lib.toggle_favorite(&id).unwrap());
    assert_eq!(lib.favorites().len(), 1);
    assert!(!lib.toggle_favorite(&id).unwrap());
    assert!(matches!(
        lib.toggle_favorite("nope"),
        Err(LibraryError::UnknownSound(_))
    ));
}

#[test]
fn import_copies_into_managed_dir_and_remove_keeps_original() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("boom.wav");
    write_fake_wav(&original);
    let managed = dir.path().join("managed");

    let e = import_file(&original, Some(&managed), ImportSource::Local).unwrap();
    assert_eq!(e.name, "boom");
    assert!(e.path.starts_with(&managed));
    assert!(e.path.exists());

    let mut lib = Library::default();
    let id = lib.add_entry(e.clone());
    let removed = lib.remove(&id).unwrap();
    assert!(lib.is_empty());
    assert!(original.exists(), "the user's original file must never be deleted");
    assert!(removed.path.exists(), "removing an entry must not delete files");

    assert!(remove_managed_file(&removed, &managed).unwrap());
    assert!(!removed.path.exists());
    assert!(original.exists());
}

#[test]
fn remove_managed_file_never_touches_foreign_files() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("mine.wav");
    write_fake_wav(&original);
    let managed = dir.path().join("managed");
    fs::create_dir_all(&managed).unwrap();
    let e = import_file(&original, None, ImportSource::Local).unwrap();
    assert!(!remove_managed_file(&e, &managed).unwrap());
    assert!(original.exists());
}

#[test]
fn import_rejects_bad_files() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.wav");
    fs::write(&empty, b"").unwrap();
    assert!(matches!(
        import_file(&empty, None, ImportSource::Local),
        Err(LibraryError::EmptyFile(_))
    ));

    assert!(matches!(
        import_file(&dir.path().join("missing.wav"), None, ImportSource::Local),
        Err(LibraryError::NotFound(_))
    ));

    let txt = dir.path().join("note.txt");
    fs::write(&txt, b"hello").unwrap();
    assert!(matches!(
        import_file(&txt, None, ImportSource::Local),
        Err(LibraryError::UnsupportedFormat(_))
    ));

    let fake = dir.path().join("fake.mp3");
    fs::write(&fake, b"<html>not audio at all</html>").unwrap();
    assert!(matches!(
        import_file(&fake, None, ImportSource::Local),
        Err(LibraryError::UnsupportedFormat(_))
    ));
}

#[test]
fn missing_file_keeps_entry_and_reports_unavailable() {
    let mut lib = Library::default();
    let id = lib.add_entry(entry("Gone"));
    let e = lib.get(&id).unwrap();
    assert!(matches!(check_available(e), Err(LibraryError::NotFound(_))));
    assert_eq!(lib.len(), 1);
}

#[test]
fn library_persists_and_survives_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut lib = Library::default();
    let id = lib.add_entry(entry("Saved"));
    lib.set_hotkey(&id, Some("ctrl+alt+k")).unwrap();
    lib.save(&path).unwrap();
    let (loaded, status) = Library::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    assert_eq!(loaded.get(&id).unwrap().hotkey.as_deref(), Some("Ctrl+Alt+K"));

    fs::write(&path, b"garbage").unwrap();
    let (recovered, status) = Library::load(&path).unwrap();
    assert!(recovered.is_empty());
    assert!(matches!(status, LoadStatus::Recovered { .. }));
}

#[test]
fn hotkey_normalization() {
    assert_eq!(normalize_hotkey("ctrl+alt+k").unwrap(), "Ctrl+Alt+K");
    assert_eq!(normalize_hotkey("K + Shift + control").unwrap(), "Ctrl+Shift+K");
    assert_eq!(normalize_hotkey("f9").unwrap(), "F9");
    assert_eq!(normalize_hotkey("win+Space").unwrap(), "Win+Space");
    assert_eq!(normalize_hotkey("ctrl+num5").unwrap(), "Ctrl+Num5");
    for bad in ["", "ctrl", "k", "ctrl+", "ctrl+a+b", "ctrl+banana", "f25"] {
        assert!(
            matches!(normalize_hotkey(bad), Err(LibraryError::InvalidHotkey(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn hotkey_conflicts_are_rejected_not_silent() {
    let mut lib = Library::default();
    let a = lib.add_entry(entry("A"));
    let b = lib.add_entry(entry("B"));
    lib.set_hotkey(&a, Some("Ctrl+1")).unwrap();
    match lib.set_hotkey(&b, Some("ctrl+1")) {
        Err(LibraryError::HotkeyConflict { owner, hotkey }) => {
            assert_eq!(owner, "A");
            assert_eq!(hotkey, "Ctrl+1");
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    assert_eq!(lib.get(&b).unwrap().hotkey, None);
    // Re-assigning the same hotkey to its owner is fine, and clearing works.
    lib.set_hotkey(&a, Some("Ctrl+1")).unwrap();
    lib.set_hotkey(&a, None).unwrap();
    lib.set_hotkey(&b, Some("Ctrl+1")).unwrap();
}

#[test]
fn sniffing_formats() {
    assert_eq!(AudioFormat::sniff(b"ID3\x04\0\0\0\0\0\0"), Some(AudioFormat::Mp3));
    assert_eq!(AudioFormat::sniff(&[0xFF, 0xFB, 0x90, 0x00]), Some(AudioFormat::Mp3));
    assert_eq!(AudioFormat::sniff(&[0xFF, 0xF1, 0x50, 0x80]), None); // AAC/ADTS is not MP3
    assert_eq!(AudioFormat::sniff(b"RIFF\0\0\0\0WAVEfmt "), Some(AudioFormat::Wav));
    assert_eq!(AudioFormat::sniff(b"RIFF\0\0\0\0AVI LIST"), None);
    assert_eq!(AudioFormat::sniff(b"<!DOCTYPE html>"), None);
}

#[test]
fn old_library_entries_without_new_fields_load_with_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    fs::write(
        &path,
        br#"{"version":1,"entries":[{"id":"a","name":"Old","path":"C:/x/a.wav","hotkey":"Ctrl+1","favorite":true}]}"#,
    )
    .unwrap();
    let (lib, status) = Library::load(&path).unwrap();
    assert_eq!(status, LoadStatus::Loaded);
    let e = lib.get("a").unwrap();
    assert_eq!((e.volume, e.content_hash), (1.0, None));
    assert_eq!(e.hotkey.as_deref(), Some("Ctrl+1"));
    assert!(e.favorite);
}

#[test]
fn damaged_entries_are_skipped_but_the_rest_survives() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    fs::write(
        &path,
        br#"{"version":1,"entries":[
            {"id":"a","name":"Good","path":"a.wav","volume":9.0},
            {"id":"b","name":"Bad","path":123},
            {"id":"a","name":"Duplicate id","path":"c.wav"},
            {"id":"d","name":"Also good","path":"d.wav"}]}"#,
    )
    .unwrap();
    let (lib, status) = Library::load(&path).unwrap();
    assert_eq!(lib.len(), 2);
    assert_eq!(lib.get("a").unwrap().volume, 2.0, "volume is clamped");
    match status {
        LoadStatus::Recovered { backup: Some(side), .. } => assert!(side.exists()),
        other => panic!("unexpected status {other:?}"),
    }
}

#[test]
fn per_sound_volume_is_clamped() {
    let mut lib = Library::default();
    let id = lib.add_entry(entry("V"));
    assert_eq!(lib.set_volume(&id, 0.5).unwrap(), 0.5);
    assert_eq!(lib.set_volume(&id, 7.0).unwrap(), MAX_ENTRY_VOLUME);
    assert_eq!(lib.set_volume(&id, -1.0).unwrap(), 0.0);
    assert_eq!(lib.set_volume(&id, f32::NAN).unwrap(), 1.0);
    assert!(matches!(
        lib.set_volume("nope", 1.0),
        Err(LibraryError::UnknownSound(_))
    ));
}

#[test]
fn identical_content_is_detected_as_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.wav");
    let b = dir.path().join("b.wav");
    write_fake_wav(&a);
    write_fake_wav(&b);
    let ea = import_file(&a, None, ImportSource::Local).unwrap();
    let eb = import_file(&b, None, ImportSource::Local).unwrap();
    assert!(ea.content_hash.is_some());
    assert_eq!(ea.content_hash, eb.content_hash);
    let mut lib = Library::default();
    lib.add_entry(ea);
    assert!(lib.find_by_hash(eb.content_hash.unwrap()).is_some());
    assert!(lib.find_by_hash(1).is_none());
}

#[test]
fn missing_ids_lists_only_unavailable_files() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.wav");
    write_fake_wav(&real);
    let mut lib = Library::default();
    let good = lib.add_entry(import_file(&real, None, ImportSource::Local).unwrap());
    let gone = lib.add_entry(entry("Gone"));
    let missing = lib.missing_ids();
    assert_eq!(missing, vec![gone]);
    assert!(!missing.contains(&good));
}
