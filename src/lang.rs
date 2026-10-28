//! UI localisation: English (reference) and Russian.
//!
//! Two mechanisms:
//! * [`t`] translates static, literal UI strings (the English text is the dictionary key).
//! * [`ru`] tells the caller that a dynamic `format!` string must be written in Russian, so that
//!   sentence structure, word order and plurals can be natural in both languages.
//!
//! Device names, file names, product names ("Discord", "VB-CABLE"), protocol scheme names
//! ("soundpad://", "https://") and window titles that double as identifiers are never translated.
//! Unknown or untranslated strings fall back to English.

/// Translate a static UI string. English source text is the dictionary key;
/// unknown or untranslated strings fall back to English.
pub fn t(lang: &str, text: &'static str) -> &'static str {
    if lang == "ru" {
        ru_lookup(text).unwrap_or(text)
    } else {
        text
    }
}

/// True when the UI language is Russian (switch wording of dynamic `format!` strings).
pub fn ru(lang: &str) -> bool {
    lang == "ru"
}

/// The Russian dictionary for static UI strings. English text in, Russian text out.
fn ru_lookup(text: &str) -> Option<&'static str> {
    Some(match text {
        // Sidebar and page titles.
        "Library" => "Библиотека",
        "Favorites" => "Избранное",
        "History" => "История",
        "Settings" => "Настройки",
        "Quit TapTone" => "Выйти из TapTone",
        // Player bar.
        "Nothing playing" => "Ничего не играет",
        "Press a sound's play button or its hotkey" => "Нажмите кнопку воспроизведения звука или его горячую клавишу",
        "Stop this sound" => "Остановить этот звук",
        "Play it again" => "Воспроизвести снова",
        "Stop All" => "Остановить все",
        "Master" => "Общая",
        "Phones" => "Науш.",
        "Mic" => "Микр.",
        "MIC OFF" => "МИК ВЫКЛ",
        "MIC MUTED" => "МИК ЗАГЛУШ",
        "MIC ON" => "МИК ВКЛ",
        "MIC !" => "МИК !",
        "The microphone route is off. Click to set it up." => "Маршрут микрофона выключен. Нажмите, чтобы настроить его.",
        "Sounds are not sent to the microphone. Click to unmute." => "Звуки не отправляются в микрофон. Нажмите, чтобы включить звук.",
        "Sounds are sent to the microphone route. Click to mute it." => "Звуки отправляются в маршрут микрофона. Нажмите, чтобы заглушить его.",
        "The microphone route has a problem. Click to open Settings." => "У маршрута микрофона проблема. Нажмите, чтобы открыть настройки.",
        // Alerts banner.
        "Headphones" => "Наушники",
        "Microphone route" => "Маршрут микрофона",
        "Your microphone" => "Ваш микрофон",
        "Audio settings" => "Настройки звука",
        // Sound list.
        "Add from URL" => "Добавить по ссылке",
        "Import" => "Импорт",
        "Check files" => "Проверить файлы",
        "Look for sound files that were moved or deleted" => "Искать файлы звуков, которые были перемещены или удалены",
        "Search sounds..." => "Поиск звуков...",
        "Local file" => "Локальный файл",
        "From URL" => "По ссылке",
        "File missing" => "Файл отсутствует",
        "Cancel" => "Отмена",
        "Dismiss" => "Скрыть",
        "Retry" => "Повторить",
        // Empty states.
        "No sounds match your search" => "Ничего не найдено по этому запросу",
        "Try a different name." => "Попробуйте другое название.",
        "No favorites yet" => "В избранном пока ничего нет",
        "Click the star on a sound to pin it here." => "Нажмите звёздочку у звука, чтобы он появился здесь.",
        "Your library is empty" => "Библиотека пуста",
        "Use Import, drop WAV/MP3 files here, or add a sound from a URL." => "Воспользуйтесь «Импортом», перетащите сюда файлы WAV/MP3 или добавьте звук по ссылке.",
        "Imported 1 sound" => "Импортирован 1 звук",
        // Row menu.
        "Rename" => "Переименовать",
        "Set hotkey..." => "Задать горячую клавишу...",
        "Clear hotkey" => "Убрать горячую клавишу",
        "Locate file..." => "Указать файл...",
        "Sound volume" => "Громкость звука",
        "Remove from library" => "Удалить из библиотеки",
        "Remove from favorites" => "Убрать из избранного",
        "Add to favorites" => "Добавить в избранное",
        "+ Hotkey" => "+ Горячая клавиша",
        "This combination could not be registered. Click to choose another." => "Это сочетание не удалось зарегистрировать. Нажмите, чтобы выбрать другое.",
        "Click to set a global hotkey" => "Нажмите, чтобы задать глобальную горячую клавишу",
        // History page.
        "Clear" => "Очистить",
        "Sounds played in this session, newest first." => "Звуки, проигранные в этом сеансе, сверху — самые свежие.",
        "Nothing played yet" => "Пока ничего не играло",
        "Sounds you play will show up here." => "Здесь появятся проигранные звуки.",
        // Settings: audio devices.
        "AUDIO DEVICES" => "АУДИОУСТРОЙСТВА",
        "Headphones or speakers" => "Наушники или колонки",
        "What you hear." => "То, что слышите вы.",
        "Test sound" => "Тестовый звук",
        "Refresh devices" => "Обновить устройства",
        "Looking for audio devices..." => "Ищем аудиоустройства...",
        "The list updates by itself when you plug in or remove a device." => "Список обновляется сам, когда вы подключаете или отключаете устройство.",
        "All devices Windows reports" => "Все устройства, которые видит Windows",
        "All devices the system reports" => "Все устройства, которые видит система",
        "Playback devices (outputs)" => "Устройства воспроизведения (выходы)",
        "Recording devices (inputs)" => "Устройства записи (входы)",
        "How to set up the microphone route" => "Как настроить маршрут микрофона",
        "VB-CABLE (free virtual audio cable)" => "VB-CABLE (бесплатный виртуальный аудиокабель)",
        // Settings: volume and playback.
        "VOLUME AND PLAYBACK" => "ГРОМКОСТЬ И ВОСПРОИЗВЕДЕНИЕ",
        "Master volume" => "Общая громкость",
        "Headphones volume" => "Громкость наушников",
        "Microphone volume" => "Громкость микрофона",
        "Muted" => "Заглушён",
        "Your voice volume" => "Громкость вашего голоса",
        "Default sound volume" => "Громкость звуков по умолчанию",
        "Overlapping sounds" => "Наложение звуков",
        "Allow several sounds at once" => "Разрешить играть нескольким звукам сразу",
        "Pressing a playing sound" => "Повторное нажатие играющего звука",
        "Restart the sound" => "Перезапустить звук",
        "Ignore the new press" => "Игнорировать новое нажатие",
        "Play on top (overlap)" => "Играть поверх (наложением)",
        // Settings: application.
        "APPLICATION" => "ПРИЛОЖЕНИЕ",
        "When the window is closed" => "При закрытии окна",
        "Minimize (hotkeys keep working)" => "Свернуть (горячие клавиши продолжают работать)",
        "Importing files" => "Импорт файлов",
        "Copy files into the TapTone folder" => "Копировать файлы в папку TapTone",
        "Removing sounds" => "Удаление звуков",
        "Also delete TapTone's own copy (never your original file)" => "Также удалять копию TapTone (ваш исходный файл не трогаем)",
        // Settings: protocol links.
        "SOUNDPAD LINKS" => "ССЫЛКИ SOUNDPAD",
        "Registered for this TapTone" => "Зарегистрировано для этого TapTone",
        "Not registered" => "Не зарегистрировано",
        "Register again" => "Зарегистрировать снова",
        "Register soundpad:// and taptone://" => "Зарегистрировать soundpad:// и taptone://",
        "Unregister" => "Отменить регистрацию",
        // Settings: hotkeys and data.
        "GLOBAL HOTKEYS" => "ГЛОБАЛЬНЫЕ ГОРЯЧИЕ КЛАВИШИ",
        "Hotkeys are not active." => "Горячие клавиши не активны.",
        "Mute / unmute the microphone route" => "Заглушить / вернуть звук маршрута микрофона",
        "DATA FOLDER" => "ПАПКА ДАННЫХ",
        // Dialogs.
        "Import sounds" => "Импорт звуков",
        "Audio (WAV, MP3)" => "Аудио (WAV, MP3)",
        "Locate the sound file" => "Указать файл звука",
        "Rename sound" => "Переименовать звук",
        "Save" => "Сохранить",
        "Add sound from URL" => "Добавить звук по ссылке",
        "Paste a direct https:// link to a WAV or MP3 file, or a soundpad:// link." => "Вставьте прямую ссылку https:// на файл WAV или MP3 либо ссылку soundpad://.",
        "Download" => "Скачать",
        "Record" => "Записать",
        "Recording..." => "Запись...",
        "Press a key combination... (Esc cancels)" => "Нажмите сочетание клавиш... (Esc — отмена)",
        "Click Record and press the combination you want. It works system-wide." => "Нажмите «Записать» и задайте нужное сочетание. Оно работает по всей системе.",
        "Enter or record a key combination first." => "Сначала введите или запишите сочетание клавиш.",
        // Device states and tags.
        "Off" => "Выкл",
        "No device selected" => "Устройство не выбрано",
        "Not selected" => "Не выбрано",
        "none" => "нет",
        "default" => "по умолчанию",
        "virtual" => "виртуальный",
        "(inactive)" => "(не работает)",
        "Sound" => "Звук",
        // Toasts.
        "Hotkey cleared" => "Горячая клавиша удалена",
        "File linked" => "Файл привязан",
        "Download cancelled" => "Скачивание отменено",
        "Microphone route works" => "Маршрут микрофона работает",
        "Microphone route test: see Settings" => "Тест маршрута микрофона: смотрите настройки",
        "Microphone route muted" => "Маршрут микрофона: звук выключен",
        "Microphone route unmuted" => "Маршрут микрофона: звук включён",
        "soundpad:// and taptone:// links now open TapTone" => "Ссылки soundpad:// и taptone:// теперь открывают TapTone",
        "Link registration removed" => "Регистрация ссылок удалена",
        "Global hotkeys are unavailable, so this hotkey is saved but inactive." => "Глобальные горячие клавиши недоступны, поэтому они сохранены, но не работают.",
        "Microphone mute" => "Заглушение микрофона",
        // Long help paragraphs (whitespace must match the app.rs literals exactly).
        "TapTone plays every sound on up to two routes at the same time: your headphones, and a microphone route that voice chats such as Discord can pick up. Output devices (playback) and input devices (microphones) are different things and are chosen separately." => "TapTone проигрывает каждый звук одновременно на одном-двух маршрутах: наушники и маршрут микрофона, который подхватывают голосовые чаты вроде Discord. Устройства вывода (воспроизведение) и устройства ввода (микрофоны) — разные вещи и выбираются отдельно.",
        "What other people hear. Play into a virtual audio cable (e.g. CABLE Input); voice chats then use its recording side as their microphone." => "То, что слышат другие. Играйте в виртуальный аудиокабель (например, CABLE Input); голосовые чаты будут использовать его устройство записи как свой микрофон.",
        "No virtual audio cable was found on this PC. Windows cannot send sound into the microphone without one. Open \"How to set up the microphone route\" below." => "На этом компьютере не найден виртуальный аудиокабель. Без него Windows не может отправить звук в микрофон. Откройте раздел «Как настроить маршрут микрофона» ниже.",
        "No virtual audio device (cable) was found on this PC. Without one sound cannot be sent into the microphone. Create a PipeWire null sink — see \"How to set up the microphone route\" below." => "На этом компьютере не найдено виртуальное аудио-устройство (кабель). Без него звук нельзя отправить в микрофон. Создайте null-sink в PipeWire — см. раздел «Как настроить маршрут микрофона» ниже.",
        "This does not look like a virtual audio cable. Sound will only reach a voice chat if something routes this device into a microphone." => "Это не похоже на виртуальный аудиокабель. Звук дойдёт до голосового чата, только если что-то направляет это устройство в микрофон.",
        "A real microphone or headset microphone. Switch this on if you want your voice and TapTone's sounds to reach Discord through the one device you choose there. Your voice goes only into the microphone route, never into your headphones. Leave it off if you already send your voice another way (for example, a PipeWire/PulseAudio loopback)." => "Настоящий микрофон или микрофон гарнитуры. Включите его, если хотите, чтобы ваш голос и звуки TapTone доходили до Discord через одно выбранное там устройство. Ваш голос попадает только в маршрут микрофона и никогда — в наушники. Оставьте выключенным, если вы уже передаёте голос иначе (например, через петлю PipeWire/PulseAudio).",
        "Mix my microphone into the microphone route" => "Подмешивать мой микрофон в маршрут микрофона",
        "Turn on the microphone route above first." => "Сначала включите маршрут микрофона выше.",
        "Send TapTone sounds to the microphone route" => "Отправлять звуки TapTone в маршрут микрофона",
        "Plays a short tone on the route. People in your voice chat may hear it." => "Проигрывает короткий тон в маршруте. Его могут услышать участники вашего голосового чата.",
        "Test microphone route" => "Проверить маршрут микрофона",
        "Testing..." => "Проверка...",
        "Plays a tone on the route and listens on the matching recording device to confirm that it arrives." => "Проигрывает тон в маршруте и слушает парное устройство записи, чтобы убедиться, что звук дошёл.",
        "Your microphone (an input device, optional)" => "Ваш микрофон (устройство ввода, необязательно)",
        "Each sound's own volume is in its \u{2022}\u{2022}\u{2022} menu. Final level = master x route x default x sound. Loud overlaps are limited instead of distorting." => "Громкость каждого звука — в его меню \u{2022}\u{2022}\u{2022}. Итоговый уровень = общая x маршрут x базовая x звук. Громкие наложения ограничиваются, а не искажаются.",
        "Register TapTone as the handler of soundpad:// and taptone:// links, so that clicking a \"play in Soundpad\" link on a website downloads the sound and plays it here. Only https links are downloaded." => "Зарегистрируйте TapTone как обработчик ссылок soundpad:// и taptone://, чтобы по клику на ссылку «play in Soundpad» на сайте звук скачивался и заигрывал здесь. Скачиваются только ссылки https.",
        _ => return None,
    })
}

/// English fragments (needles) of dynamically composed messages, each with a Russian replacement.
///
/// The needles come from core modules (errors.rs, hotkeys.rs, system.rs, engine.rs, routing.rs,
/// main.rs notices) whose messages reach the user inside translated frames. Sentences with an
/// embedded variable (device name, file name, hotkey, URL, number) are covered by fragments that
/// exclude the variable; the variable itself passes through untouched. Device names, file names,
/// "CABLE Input/Output", protocol schemes, registry paths and numbers are never translated.
/// The table is normalized to longest-first order at first use, so a longer, more specific
/// phrase always wins before a shorter, overlapping one.
static ERROR_TRANSLATIONS: &[(&str, &str)] = &[
    // Audio devices and route states (RouteState::describe and engine messages that embed it).
    ("Headphones is not active: ", "Наушники не активны: "),
    ("Microphone route is not active: ", "Маршрут микрофона не активен: "),
    ("The microphone route is not active: ", "Маршрут микрофона не активен: "),
    (" is not active: ", " не активен: "),
    (
        "Windows may be blocking microphone access for desktop apps (Settings > Privacy & security > Microphone > Let desktop apps access your microphone).",
        "Возможно, Windows блокирует доступ к микрофону для классических приложений (Параметры > Конфиденциальность и защита > Микрофон > Разрешить классическим приложениям доступ к микрофону).",
    ),
    (
        "Windows may be blocking microphone access for desktop apps (Settings > Privacy > Microphone).",
        "Возможно, Windows блокирует доступ к микрофону для классических приложений (Параметры > Конфиденциальность > Микрофон).",
    ),
    ("Works: the test signal came out of", "Работает: тестовый сигнал вышел на"),
    (" dBFS). Choose ", " дБ). Выберите "),
    (" as the microphone in your voice chat.", " в голосовом чате в качестве микрофона."),
    ("No signal on", "Нет сигнала на"),
    (
        ". Make sure the volume of the microphone route is above 0 and it is not muted, and that ",
        ". Убедитесь, что громкость маршрута микрофона выше 0, маршрут не заглушен и ",
    ),
    (" really is the playback side of that device.", " действительно является стороной воспроизведения этого устройства."),
    ("The test sound was played on", "Тестовый звук был проигран на"),
    (
        ", but TapTone could not find the recording device that belongs to it, so it cannot confirm that the sound reaches the microphone. Look for the matching \"Output\" device in your voice chat's microphone list.",
        ", но TapTone не нашёл устройство записи, которое к нему относится, поэтому не удалось подтвердить, что звук доходит до микрофона. Найдите в списке микрофонов голосового чата парное устройство \"Output\".",
    ),
    (", but listening on ", ", но при прослушивании "),
    (" failed (", " не удалось ("),
    ("Playing on ", "Играет через "),
    (" is not available, playing on ", " недоступно, играет через "),
    ("\" instead", "."),
    (" stopped working (", " перестало работать ("),
    ("). TapTone is trying to reconnect.", "). TapTone пытается восстановить связь."),
    (" is not connected. TapTone keeps looking for it.", " не подключено. TapTone продолжает искать."),
    (" is also the headphones output. Choose a different device.", " — это и выход наушников. Выберите другое устройство."),
    ("The device cannot be used: ", "Устройство нельзя использовать: "),
    ("Your microphone: ", "Ваш микрофон: "),
    ("Headphones: ", "Наушники: "),
    ("Microphone route: ", "Маршрут микрофона: "),
    ("No audio output is open (", "Не открыто ни одного устройства вывода ("),
    ("). Check Settings.", "). Проверьте настройки."),
    ("Too many sounds are playing at once.", "Одновременно играет слишком много звуков."),
    (
        " is the recording side of the microphone route, so using it would feed the sound back into itself. Choose your real microphone.",
        " — сторона записи маршрута микрофона; через него звук уходил бы обратно в сам маршрут. Выберите свой настоящий микрофон.",
    ),
    ("cannot read the format of \"", "не удалось прочитать формат \""),
    ("cannot open \"", "не удалось открыть \""),
    ("cannot start \"", "не удалось запустить \""),
    ("unsupported sample format", "неподдерживаемый формат отсчётов"),
    ("no audio output device is available", "нет доступного устройства вывода"),
    ("no recording device (microphone) was found", "не найдено устройство записи (микрофон)"),
    ("Could not start the audio engine: ", "Не удалось запустить звуковой движок: "),
    ("Could not start the test: ", "Не удалось запустить проверку: "),
    ("Cannot open the file: ", "Не удалось открыть файл: "),
    ("Unsupported or damaged audio file: ", "Аудиофайл не поддерживается или повреждён: "),
    ("The file contains no audio track", "В файле нет звуковой дорожки"),
    ("Unsupported audio format: ", "Неподдерживаемый звуковой формат: "),
    ("Decoding failed: ", "Ошибка декодирования: "),
    ("The sound is longer than ", "Звук длиннее, чем "),
    (" minutes, which is not supported", " минут, что не поддерживается"),
    ("The file contains no audio data", "В файле нет звуковых данных"),
    // lib errors.
    ("Unsupported file type: ", "Неподдерживаемый тип файла: "),
    ("The file is empty: ", "Файл пуст: "),
    ("File not found or not a regular file: ", "Файл не найден или это не обычный файл: "),
    (" is already used by \"", " уже используется звуком \""),
    ("Invalid hotkey: ", "Некорректная горячая клавиша: "),
    ("No sound with id ", "Нет звука с id "),
    ("File access error at ", "Ошибка доступа к файлу "),
    ("Could not read or write JSON at ", "Не удалось прочитать или записать JSON "),
    // Protocol links.
    ("This is not a valid TapTone/Soundpad link: ", "Это недопустимая ссылка TapTone/Soundpad: "),
    ("Unsupported link scheme: ", "Неподдерживаемая схема ссылки: "),
    ("Unsupported command: ", "Неподдерживаемая команда: "),
    ("The link does not contain a sound URL", "В ссылке нет адреса звука"),
    ("The sound URL in the link is invalid: ", "Адрес звука в ссылке недопустим: "),
    ("Only https:// sound URLs are allowed (got ", "Разрешены только адреса звука с https:// (получено "),
    // Downloads.
    ("Invalid URL: ", "Недопустимая ссылка: "),
    ("Only https:// URLs are allowed (got ", "Разрешены только ссылки на https:// (получено "),
    ("URLs with a user name or password are not allowed", "Ссылки с именем пользователя или паролем не разрешены"),
    ("Address ", "Адрес "),
    (" is local or private and cannot be downloaded from", " локальный или приватный, скачивание с него невозможно"),
    ("Local host names are not allowed", "Локальные имена узлов не разрешены"),
    ("Could not resolve host ", "Не удалось найти хост "),
    ("Too many redirects", "Слишком много перенаправлений"),
    ("The server sent an invalid redirect", "Сервер прислал некорректное перенаправление"),
    ("The server answered with HTTP status ", "Сервер ответил кодом HTTP "),
    ("The file is larger than the ", "Файл больше, чем "),
    (" MiB limit", " МиБ (ограничение)"),
    ("The download timed out", "Скачивание не уложилось в срок"),
    ("The download was cancelled", "Скачивание отменено"),
    ("This link points to a web page, not directly to an audio file", "Эта ссылка ведёт на веб-страницу, а не сразу на аудиофайл"),
    ("The downloaded data is not a supported audio file (WAV or MP3)", "Скачанные данные не являются поддерживаемым аудиофайлом (WAV или MP3)"),
    ("Network error: ", "Сетевая ошибка: "),
    ("File error: ", "Ошибка файла: "),
    // Hotkeys, registry and startup notices.
    ("This key combination is already used by another program or cannot be registered (", "Это сочетание уже используется другой программой или его нельзя зарегистрировать ("),
    ("Unsupported key in \"", "Неподдерживаемая клавиша в \""),
    ("Global hotkeys are unavailable: ", "Глобальные горячие клавиши недоступны: "),
    ("Cannot find the TapTone executable: ", "Не удалось найти исполняемый файл TapTone: "),
    ("Could not write to the registry: ", "Не удалось записать в реестр: "),
    ("Could not remove the registry entry: ", "Не удалось удалить запись в реестре: "),
    ("Protocol registration is not supported on this system", "Регистрация протокола недоступна в этой системе"),
    (
        ". Check that the input device is not muted and that the sound service (PipeWire/PulseAudio) is running.",
        ". Проверьте, что устройство ввода не заглушено, а звуковой сервис (PipeWire/PulseAudio) работает.",
    ),
    (
        ". Check that the input device is not busy or muted and that the sound service (PipeWire/PulseAudio) is running.",
        ". Проверьте, что устройство ввода не занято и не заглушено, а звуковой сервис (PipeWire/PulseAudio) работает.",
    ),
    (": only one non-modifier key is allowed", ": допускается только одна клавиша без модификатора"),
    (": add a modifier such as Ctrl or Alt", ": добавьте модификатор, например Ctrl или Alt"),
    (": empty key name", ": пустое имя клавиши"),
    (": unknown key", ": неизвестная клавиша"),
    (": no key given", ": не указана клавиша"),
    ("Could not find the per-user data folder; using a temporary folder instead.", "Не удалось найти пользовательскую папку данных; вместо неё используется временная папка."),
    ("Could not create ", "Не удалось создать "),
    ("settings.json had a problem and defaults were used (", "В settings.json проблема, использованы значения по умолчанию ("),
    ("library.json had a problem (", "В library.json проблема ("),
    (" A copy was kept at ", " Копия сохранена в "),
    (" The unreadable data was kept at ", " Нечитаемые данные сохранены в "),
    ("Waiting for the microphone route to start.", "Ожидание запуска маршрута микрофона."),
];

/// The error table in longest-first order (built once on first use).
fn error_table() -> &'static [(&'static str, &'static str)] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: Vec<(&'static str, &'static str)> = ERROR_TRANSLATIONS.to_vec();
        table.sort_by_key(|(needle, _)| std::cmp::Reverse(needle.len()));
        table
    })
}

/// Translate a dynamically composed message (typically an error body from a core module)
/// into Russian by replacing known English phrases. Unknown text passes through unchanged,
/// so nothing is lost and no English-language regression happens for `lang != "ru"`.
pub fn tx(lang: &str, text: &str) -> String {
    if lang != "ru" {
        return text.to_string();
    }
    let mut out = text.to_string();
    for (needle, replacement) in error_table() {
        if out.contains(needle) {
            out = out.replace(needle, replacement);
        }
    }
    out
}
