# TapTone — PROGRESS

Обновлено: 2026-10-27. Эпоха «собственного драйвера/APO» закрыта (см. раздел 2):
приложение переработано под стратегию из промта «полная переработка» — без собственного
аудиодрайвера, только штатные системные средства маршрутизации, подготовка к публикации.

## 1. Сводный статус

| Направление | Статус |
|---|---|
| Удаление драйверного пути (native APO, ffi, capture_apo) | **Выполнено и проверено** (см. раздел 2) |
| Windows: рабочее приложение + маршрутизация через виртуальный кабель | **Реализовано и проверено** (предыдущие сессии; инструмент `examples/level3_cable_loopback.rs`, логи `build/logs/cable_loopback*.log`) |
| GUI (зелёно-чёрная тема, карточки, настройки, тосты) | **Реализовано** (проверено юнит-тестами; визуальная приёмка остаётся за человеком) |
| Локализация RU/EN | **Выполнено и проверено** (147 словарных строк `lang::t` + ~40 билингв. динамических + 98 подстановок ошибок ядра `lang::tx`; переключатель в настройках; 9 тестов `tests/lang_tests.rs`) |
| CI (fmt/check/test/clippy, windows+ubuntu) | **Подготовлено** (`.github/workflows/ci.yml`; запуск на реальном GitHub — нужен владелец) |
| Release-сборки в CI (windows/Linux артефакты) | **Подготовлено** (`.github/workflows/release.yml`, публикация — только вручную владельцем) |
| PKGBUILD (AUR) / debian / rpm | **Подготовлено, НЕ собрано** (см. `packaging/README.md`, честные причины) |
| AppImage | **Не создан — осознанно** (нет Linux-окружения для честной проверки) |
| Linux runtime приложения | **NOT TESTED** (компилируется из того же кода; desktop-проверка не выполнялась) |
| Лицензия MIT, README, CHANGELOG, шаблоны issue | **Выполнено** |
| Иконка Linux (`assets/icon.png` 256px, из `AvtrV.ico`) | **Выполнено** |
| Windows-сборка приложения (release) | **Выполнена** (exit 0; exe скопирован пользователю на рабочий стол) |
| Git-репозиторий (init/commit/push) | **НЕ выполнено** — git в системе не установлен; публикация — действие владельца |

## 2. Удаление драйверной архитектуры (эта сессия)

Решение проекта: TapTone не разрабатывает, не собирает, не устанавливает и не поставляет
собственный аудиодрайвер или системный аудиокомпонент. Экспериментальный capture-APO маршрут
(подмешивание через подписанный системный пакет) был оценён и удалён.

### Удалено (файлами)

- `native/apo/**` — COM APO DLL (`taptone_apo.dll`), component/extension INF, 7
  PowerShell-скриптов сборки/подписи/регистрации пакета, mock-заголовки Windows, COM-тесты.
- `native/` целиком: C++-ядро `taptone_core` (`src/abi.cpp`, `include/*.h`),
  `build_apo.cmd`, `check.sh`, `CMakeLists.txt` (APO-цели и цели ядра), тесты ядра/APO.
- `src/ffi/**`, `src/audio/apo_ipc.rs`, `tests/abi_layout_tests.rs`, `tests/abi_native_tests.rs`.
- `docs/APO_ARCHITECTURE.md`, `docs/APO_INSTALL.md`, `docs/ABI_CONTRACT.md`,
  `docs/VIRTUAL_MIC.md` (заменён на `docs/AUDIO_ROUTING.md`).
- App-side: флаги `capture_apo_*` в `settings.rs`/`engine.rs`/`app.rs`, `Want::CaptureApo*`,
  `open_capture_apo`, `build_capture_activation_stream`, `apo_voice_gain`, GUI-чекбоксы APO,
  Cargo-фича `native`, ветка native в `build.rs`.

### Политика удаления (проверена)

- Запросы `grep` после удаления: `src/`, `tests/`, `examples/`, `Cargo.toml`, `build.rs` не
  содержат ссылок на APO/ffi/native (осталось только легитимное: `std::ffi::OsString`,
  `eframe::run_native`, тест совместимости старых settings.json).
- Обратная совместимость настроек: старые `settings.json` с полями `capture_apo_*` загружаются,
  поля игнорируются — закреплено тестом `capture_apo_values_in_old_settings_are_ignored`.
- Резервные копии ВСЕГО удалённого/правленного — `backups/driver_removal/` (+ ранние копии в
  `backups/*.bak`).

### Проверки этой сессии (реальные коды выхода)

```
cargo fmt --all          → EXITCODE=0
cargo test --all-targets → 132 теста ok (сразу после удаления; ABI-наборы ушли вместе с ffi)
```

Финальное состояние (после локализации и правки clippy):

```
cargo fmt --all -- --check → EXITCODE=0
cargo test --all-targets   → 141 тест ok, EXITCODE=0
  (37 audio/unit, 9 downloader, 9 lang, 17 library, 13 live input,
   17 mixer, 26 protocol, 13 settings)
cargo clippy --all-targets --locked -- -D warnings → EXITCODE=0 (0 предупреждений;
  20 механических замечаний clippy исправлены: map_or→is_some_and/is_none_or,
  sort_by→sort_by_key, needless borrow/clone, manual_ok_err)
cargo build --release → EXITCODE=0 → target\release\taptone.exe (10 455 040 байт)
```

### Локализация RU/EN (эта сессия)

- `src/lang.rs`: `t()` — словарь из 147 статических строк интерфейса; `ru()`; `tx()` —
  98 needle-замен для тел ошибок ядра (library, downloader, system, hotkeys, protocol,
  engine, routing, main-notices), перевод только на границе отображения.
- `src/app.rs`: все видимые строки через `lang::t`/билингвальные `format!`; динамические
  статусы (`route_status`, `voice_status`, `explain_devices` и др.); переключатель Language
  (English/Русский) в настройках, автосохранение.
- Тесты: `tests/lang_tests.rs` (9 тестов, включая идемпотентность `tx`).
- Не переведено осознанно: имена устройств/файлов, протоколы `soundpad://`/`taptone://`,
  хоткей-нотация, `id_source`, встроенные тексты ОС (`Access is denied. (os error 5)`),
  cpal/reqwest/symphonia, tracing-only сообщения.

## 3. Аудит (этап 1) — краткая сводка

- Аудио: `mixer.rs` (чистый Rust: сумма голосов, лимитер, soft-clip, LiveInput), `engine.rs`
  (worker-поток, два маршрута, ресемпл по устройствам), `routing.rs` (спаривание кабелей,
  защита от фидбэка, тест маршрута) — всё сохранилось и покрыто тестами.
- GUI: `theme.rs` — уже зелёно-чёрная неоновая тема (BG `#090B0A`, ACCENT `#39FF78`, карточки,
  иконки, nav) — менять не требовалось; `app.rs` — страницы Page, тосты, диалоги, history.
- Безопасные механизмы уже были: hotkeys через `global-hotkey`, регистрация протокола только
  HKCU (без администратора), пути XDG (`directories`), downloader c SSRF-защитой (тесты есть),
  tolerant settings/library.
- Проблемы, найденные аудитом: драйверное наследие (см. раздел 2), README с инструкциями
  APO-режима и «код написан без компилятора», отсутствие LICENSE/CHANGELOG/CI/упаковки,
  отсутствие локализации (только поле `language`), отсутствие desktop entry/icon для Linux.

## 4. Аудиоархитектура (итоговая)

```
файлы (WAV/MP3) → symphonia → ресемпл → Rust-mixer (голоса + LiveInput-микрофон)
                      ├→ Master + Main gain  → headphones (локальный режим)
                      └→ Master + Mic gain   → «маршрут микрофона»
Windows: маршрут микрофона → playback виртуального кабеля («CABLE Input»);
         голосовые чаты берут парный capture («CABLE Output»).
Linux:   тот же маршрут → пользовательский PipeWire/PulseAudio loopback → виртуальный source.
```

- Драйвер не нужен и не поставляется; отсутствие кабеля/loopback — ясное состояние
  «маршрут недоступен», локальное воспроизведение продолжает работать.
- Проверка уровня 3 (Windows, вариант A): тон движка доходит до capture endpoint кабеля и
  записывается независимым клиентом (`examples/level3_cable_loopback.rs`, exit 0; логи и WAV в
  `build/logs/`). Финальная проверка смеси голос+звуки «на слух» — за пользователем.

## 5. Упаковка и публикация (этапы 4–6)

- `packaging/README.md` — сводная таблица честных статусов (что собрано, что нет и почему).
- `packaging/arch/PKGBUILD` — рецепт AUR; контрольные суммы генерируются после первого тега
  (никаких фиктивных сумм), `makepkg` в среде недоступен — тестирование на Arch не выполнялось.
- `packaging/debian/{control,rules}` + `packaging/rpm/taptone.spec` — подготовлены, не собраны.
- `packaging/linux/taptone.desktop`, `assets/icon.png` — готовы.
- `.github/workflows/ci.yml`: fmt, test, clippy `-D warnings`, windows+ubuntu, без аудио-железа.
- `.github/workflows/release.yml`: только артефакты; публикация релиза — вручную владельцем.
- `.github/ISSUE_TEMPLATE/{bug_report.yml,feature_request.yml}`.

## 6. Сборка и доставка

```sh
cargo fmt --all && cargo test --all-targets && cargo build --release
# Windows: target\release\taptone.exe (иконка вшита build.rs/winres)
# Linux:   target/release/taptone  (нужны libasound2, libxkbcommon(-x11), libx11/wayland)
```

Скопировано пользователю на рабочий стол (по его просьбе, 2026-10-27):
- `Desktop\taptone\` — свежий `taptone.exe` (обновлена прежняя сборка), README, LICENSE,
  CHANGELOG, PROGRESS;
- `Desktop\TapTone-source\` — исходный проект без `target`/`build`/`backups` (63 файла,
  9.3 МБ) + `build\logs` с логами и WAV доказательств проверки маршрута.

## 7. Осталась работа (не выполнено здесь, честно)

1. Git в системе не установлен: `git init`/commit/push и публикация на GitHub — действие
   владельца (или после установки git). Плейсхолдеры `GITHUB_OWNER` в packaging-файлах.
2. Реальная проверка CI на GitHub (fmt/test/clippy на windows-latest/ubuntu-latest) —
   после первого push.
3. Linux: собрать и прогнать на реальной системе; только после этого подтверждать поддержку
   PipeWire-маршрута по факту и публиковать пакеты (AUR: updpkgsums → makepkg → .SRCINFO).
4. Убрать остатки драйверной эпохи из `build\` (~1.3 ГБ: WDK-выгрузки, `release-staged`,
   apo-package*) — не удаляемые вслепую, решение за владельцем; `build\logs` сохранён.
5. Пользовательская проверка Windows-сборки: запустить exe, проверить языки RU/EN, горячие
   клавиши и маршрут с реальным Discord; прослушать `build\logs\cable_loopback_mic_*.wav`.
