//! Operating-system integration:
//!
//! * registration of the `soundpad://` and `taptone://` URL protocols for the current user (Windows registry,
//!   `HKCU\Software\Classes`, no administrator rights needed);
//! * single-instance support. A browser starts a new process for every `soundpad://` link, so the
//!   new process hands the link to the already running TapTone over a loopback socket and exits.

#[cfg_attr(not(windows), allow(unused_imports))]
use crate::protocol::{has_supported_scheme, SCHEMES};
use crossbeam_channel::Sender;
use eframe::egui;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Lets background threads wake the GUI. Filled in once the egui context exists.
pub type WakeHook = Arc<Mutex<Option<egui::Context>>>;

/// Ask the GUI to repaint (and therefore process pending events).
pub fn wake(hook: &WakeHook) {
    if let Ok(guard) = hook.lock() {
        if let Some(ctx) = guard.as_ref() {
            ctx.request_repaint();
        }
    }
}

// ---- Single instance -------------------------------------------------------------------------

pub const IPC_ADDR: &str = "127.0.0.1:47653";
/// Message sent by a second launch without a link: "bring your window to the front".
pub const SHOW_MESSAGE: &str = "taptone://show";
const IPC_GREETING: &str = "TAPTONE-OK";
const MAX_IPC_LINE: u64 = 8192;

pub enum Instance {
    /// This process is the (only) running TapTone. `listener` is `None` if the port was unusable;
    /// links are then simply not forwarded from other launches.
    Primary { listener: Option<TcpListener> },
    /// Another TapTone is running and has received our message. This process should exit.
    Forwarded,
}

fn ipc_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 47653))
}

fn send_to_running(message: &str) -> io::Result<()> {
    let mut stream = TcpStream::connect_timeout(&ipc_addr(), Duration::from_millis(800))?;
    stream.set_read_timeout(Some(Duration::from_millis(1500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(1500)))?;
    stream.write_all(message.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut reply = String::new();
    BufReader::new(&stream).take(64).read_line(&mut reply)?;
    if reply.trim() == IPC_GREETING {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the port is used by another program",
        ))
    }
}

/// Become the primary instance, or forward `message` (a protocol link, or a "show window" request)
/// to the instance that already runs.
pub fn acquire_instance(link: Option<&str>) -> Instance {
    match TcpListener::bind(ipc_addr()) {
        Ok(listener) => Instance::Primary {
            listener: Some(listener),
        },
        Err(bind_err) => {
            tracing::info!("another instance may be running ({bind_err}); forwarding");
            match send_to_running(link.unwrap_or(SHOW_MESSAGE)) {
                Ok(()) => Instance::Forwarded,
                Err(e) => {
                    tracing::warn!("could not reach a running instance: {e}");
                    Instance::Primary { listener: None }
                }
            }
        }
    }
}

fn is_ours(line: &str) -> bool {
    has_supported_scheme(line)
}

/// Accept messages from later launches and pass them to the GUI through `tx`.
/// Only `soundpad:` / `taptone:` lines are accepted; anything else is dropped.
pub fn start_listener(listener: TcpListener, tx: Sender<String>, hook: WakeHook) {
    let spawned = std::thread::Builder::new()
        .name("taptone-ipc".to_string())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                let read = BufReader::new((&stream).take(MAX_IPC_LINE)).read_line(&mut line);
                if read.is_err() || !is_ours(&line) {
                    continue;
                }
                let _ = stream.write_all(format!("{IPC_GREETING}\n").as_bytes());
                if tx.send(line.trim().to_string()).is_err() {
                    break;
                }
                wake(&hook);
            }
        });
    if let Err(e) = spawned {
        tracing::error!("could not start the IPC listener: {e}");
    }
}

// ---- soundpad:// registration ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolState {
    NotRegistered,
    /// Registered and pointing at this executable.
    Registered,
    /// Registered, but for a different program or location (the command is included).
    RegisteredElsewhere(String),
    /// Some schemes point at this executable, the named ones are still missing.
    Partial(String),
}

#[cfg(windows)]
mod platform {
    use super::{ProtocolState, SCHEMES};
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    fn key_path(scheme: &str) -> String {
        format!(r"Software\Classes\{scheme}")
    }

    fn exe_command() -> Result<String, String> {
        let exe = std::env::current_exe().map_err(|e| format!("Cannot find the TapTone executable: {e}"))?;
        Ok(format!("\"{}\" \"%1\"", exe.display()))
    }

    fn registered_command(scheme: &str) -> Option<String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey(format!(r"{}\shell\open\command", key_path(scheme)))
            .and_then(|k| k.get_value::<String, _>(""))
            .ok()
    }

    pub fn state() -> ProtocolState {
        let mine = exe_command().ok();
        let mut missing: Vec<&str> = Vec::new();
        let mut ours = 0;
        for scheme in SCHEMES {
            match registered_command(scheme) {
                None => missing.push(scheme),
                Some(cmd) => {
                    if mine.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(&cmd)) {
                        ours += 1;
                    } else {
                        return ProtocolState::RegisteredElsewhere(format!("{scheme}: {cmd}"));
                    }
                }
            }
        }
        if ours == SCHEMES.len() {
            ProtocolState::Registered
        } else if ours == 0 {
            ProtocolState::NotRegistered
        } else {
            ProtocolState::Partial(missing.join(", "))
        }
    }

    pub fn register() -> Result<(), String> {
        let command = exe_command()?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let io = |e: std::io::Error| format!("Could not write to the registry: {e}");
        for scheme in SCHEMES {
            let (root, _) = hkcu.create_subkey(key_path(scheme)).map_err(io)?;
            root.set_value("", &format!("URL:{scheme} Protocol")).map_err(io)?;
            root.set_value("URL Protocol", &"").map_err(io)?;
            let (icon, _) = root.create_subkey("DefaultIcon").map_err(io)?;
            icon.set_value("", &format!("\"{}\",0", exe.display())).map_err(io)?;
            let (cmd, _) = root.create_subkey(r"shell\open\command").map_err(io)?;
            cmd.set_value("", &command).map_err(io)?;
        }
        Ok(())
    }

    pub fn unregister() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for scheme in SCHEMES {
            match hkcu.delete_subkey_all(key_path(scheme)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Could not remove the registry entry: {e}")),
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::ProtocolState;

    pub fn state() -> ProtocolState {
        ProtocolState::NotRegistered
    }

    pub fn register() -> Result<(), String> {
        Err("Protocol registration is not supported on this system".to_string())
    }

    pub fn unregister() -> Result<(), String> {
        Err("Protocol registration is not supported on this system".to_string())
    }
}

/// Current registration state of `soundpad://` and `taptone://`.
pub fn protocol_state() -> ProtocolState {
    platform::state()
}

/// Register `soundpad://` and `taptone://` links to open this TapTone (current user only).
pub fn register_protocol() -> Result<(), String> {
    platform::register()
}

/// Remove the protocol registrations made by TapTone.
pub fn unregister_protocol() -> Result<(), String> {
    platform::unregister()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_protocol_lines_are_accepted() {
        assert!(is_ours("soundpad://play?url=https%3A%2F%2Fexample.com%2Fa.mp3"));
        assert!(is_ours("  TAPTONE://show "));
        assert!(!is_ours("GET / HTTP/1.1"));
        assert!(!is_ours(""));
        assert!(!is_ours("http://example.com"));
    }
}
