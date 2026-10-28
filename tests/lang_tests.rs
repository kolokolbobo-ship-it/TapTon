//! Tests for `lang::tx` — translation of dynamically composed error bodies from core modules.
//!
//! The needles are real strings from `errors.rs`, `hotkeys.rs`, `system.rs`, `engine.rs`,
//! `routing.rs` and the startup notices of `main.rs`.

use taptone::lang::tx;

#[test]
fn english_passes_through_unchanged() {
    assert_eq!(tx("en", "Disconnected"), "Disconnected");
    assert_eq!(
        tx("en", "Could not write to the registry: Access is denied. (os error 5)"),
        "Could not write to the registry: Access is denied. (os error 5)"
    );
}

#[test]
fn unknown_russian_text_passes_through_unchanged() {
    let own = "Приложение сообщает о странном событии «123» — #!?";
    assert_eq!(tx("ru", own), own);
    // Text that merely surrounds known needles without containing one stays untouched as well.
    let own2 = "Сочетание Ctrl+Alt+P очень странное";
    assert_eq!(tx("ru", own2), own2);
}

#[test]
fn registry_error_is_translated() {
    assert_eq!(
        tx("ru", "Could not write to the registry: Access is denied. (os error 5)"),
        "Не удалось записать в реестр: Access is denied. (os error 5)"
    );
}

#[test]
fn global_hotkeys_error_is_translated() {
    assert_eq!(
        tx("ru", "Global hotkeys are unavailable: Access is denied (os error 5)"),
        "Глобальные горячие клавиши недоступны: Access is denied (os error 5)"
    );
}

#[test]
fn hotkey_conflict_fragment_is_translated() {
    // `LibraryError::HotkeyConflict`: the hotkey and the owner name are variables in between.
    assert_eq!(
        tx("ru", "Hotkey Ctrl+Alt+K is already used by \"alarm\""),
        "Hotkey Ctrl+Alt+K уже используется звуком \"alarm\""
    );
}

#[test]
fn download_error_is_translated() {
    assert_eq!(tx("ru", "Too many redirects"), "Слишком много перенаправлений");
    assert_eq!(
        tx("ru", "The downloaded data is not a supported audio file (WAV or MP3)"),
        "Скачанные данные не являются поддерживаемым аудиофайлом (WAV или MP3)"
    );
}

#[test]
fn device_failure_chain_is_translated() {
    // A `RouteState::Failed` reason shown through `RouteState::describe` and the
    // microphone-privacy hint appended by `add_input_hint`.
    let english = concat!(
        "Your microphone: ",
        "The device cannot be used: ",
        "cannot open \"Microphone (Realtek)\": Access is denied. (0x80070005). ",
        "Windows may be blocking microphone access for desktop apps ",
        "(Settings > Privacy & security > Microphone > Let desktop apps access your microphone)."
    );
    let russian = concat!(
        "Ваш микрофон: ",
        "Устройство нельзя использовать: ",
        "не удалось открыть \"Microphone (Realtek)\": Access is denied. (0x80070005). ",
        "Возможно, Windows блокирует доступ к микрофону для классических приложений ",
        "(Параметры > Конфиденциальность и защита > Микрофон > Разрешить классическим приложениям доступ к микрофону)."
    );
    assert_eq!(tx("ru", english), russian);
}

#[test]
fn storage_error_is_translated() {
    assert_eq!(
        tx(
            "ru",
            "Could not read or write JSON at \"C:\\data\\library.json\": expected value at line 3"
        ),
        "Не удалось прочитать или записать JSON \"C:\\data\\library.json\": expected value at line 3"
    );
}

#[test]
fn translation_is_idempotent() {
    let once = tx("ru", "Network error: connection refused");
    assert_eq!(once, "Сетевая ошибка: connection refused");
    assert_eq!(tx("ru", &once), once);

    let chain = tx(
        "ru",
        "Your microphone: \"Mic\" stopped working (Access is denied. (0x80070005)). Windows may be \
         blocking microphone access for desktop apps (Settings > Privacy & security > Microphone > \
         Let desktop apps access your microphone).",
    );
    assert_eq!(tx("ru", &chain), chain);
}
