//! TapTone entry point: prepares data, makes sure only one instance runs, then opens the egui window.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crossbeam_channel::unbounded;
use std::sync::{Arc, Mutex};
use taptone::app::{Startup, TapToneApp};
use taptone::downloader;
use taptone::fsutil::LoadStatus;
use taptone::library::Library;
use taptone::paths::AppPaths;
use taptone::protocol::find_uri_arg;
use taptone::settings::Settings;
use taptone::system::{self, Instance, WakeHook};

fn init_logging(paths: &AppPaths) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("taptone=info"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.logs_dir.join("taptone.log"));
    let result = match file {
        Ok(f) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(Mutex::new(f))
            .try_init(),
        Err(_) => tracing_subscriber::fmt().with_env_filter(filter).try_init(),
    };
    if let Err(e) = result {
        eprintln!("logging was already initialized: {e}");
    }
}

fn main() -> eframe::Result<()> {
    let mut notices: Vec<String> = Vec::new();

    let paths = match AppPaths::discover() {
        Some(p) => p,
        None => {
            notices.push("Could not find the per-user data folder; using a temporary folder instead.".to_string());
            AppPaths::from_root(std::env::temp_dir().join("TapTone"))
        }
    };
    if let Err(e) = paths.ensure_dirs() {
        notices.push(format!("Could not create {}: {e}", paths.root().display()));
    }
    init_logging(&paths);

    // A browser starts a new process for every soundpad:// link: hand it to the running TapTone.
    let initial_uri = find_uri_arg(std::env::args_os().skip(1));
    let listener = match system::acquire_instance(initial_uri.as_deref()) {
        Instance::Forwarded => return Ok(()),
        Instance::Primary { listener } => listener,
    };
    // Only the primary instance may clean up, otherwise it could delete another instance's download.
    downloader::cleanup_partials(&paths.sounds_dir);

    let settings = match Settings::load(&paths.settings_file) {
        Ok((s, status)) => {
            if let LoadStatus::Recovered { reason, backup } = status {
                let kept = backup
                    .map(|b| format!(" A copy was kept at {}.", b.display()))
                    .unwrap_or_default();
                notices.push(format!(
                    "settings.json had a problem and defaults were used ({reason}).{kept}"
                ));
            }
            s
        }
        Err(e) => {
            notices.push(e.to_string());
            Settings::default()
        }
    };
    let library = match Library::load(&paths.library_file) {
        Ok((l, status)) => {
            if let LoadStatus::Recovered { reason, backup } = status {
                let kept = backup
                    .map(|b| format!(" The unreadable data was kept at {}.", b.display()))
                    .unwrap_or_default();
                notices.push(format!("library.json had a problem ({reason}).{kept}"));
            }
            l
        }
        Err(e) => {
            notices.push(e.to_string());
            Library::default()
        }
    };

    let wake: WakeHook = Arc::new(Mutex::new(None));
    let (ipc_tx, ipc_rx) = unbounded::<String>();
    if let Some(listener) = listener {
        system::start_listener(listener, ipc_tx, Arc::clone(&wake));
    }

    let startup = Startup {
        paths,
        settings,
        library,
        notices,
        initial_uri,
        ipc_rx,
        wake,
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("TapTone")
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "TapTone",
        options,
        Box::new(move |cc| Ok(Box::new(TapToneApp::new(cc, startup)))),
    )
}
