//! Typed errors shared by all subsystems. Messages are written for end users;
//! technical details go to the log.

use std::net::IpAddr;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("File access error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Could not read or write JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("Unsupported file type: {0}")]
    UnsupportedFormat(String),
    #[error("The file is empty: {0}")]
    EmptyFile(PathBuf),
    #[error("File not found or not a regular file: {0}")]
    NotFound(PathBuf),
    #[error("Hotkey {hotkey} is already used by \"{owner}\"")]
    HotkeyConflict { hotkey: String, owner: String },
    #[error("Invalid hotkey: {0}")]
    InvalidHotkey(String),
    #[error("No sound with id {0}")]
    UnknownSound(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    #[error("This is not a valid TapTone/Soundpad link: {0}")]
    NotAUri(String),
    #[error("Unsupported link scheme: {0}")]
    UnsupportedScheme(String),
    #[error("Unsupported command: {0}")]
    UnsupportedCommand(String),
    #[error("The link does not contain a sound URL")]
    MissingUrl,
    #[error("The sound URL in the link is invalid: {0}")]
    InvalidTargetUrl(String),
    #[error("Only https:// sound URLs are allowed (got {0}://)")]
    InsecureScheme(String),
}

#[derive(Debug, Clone, Error)]
pub enum DownloadError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Only https:// URLs are allowed (got {0}://)")]
    UnsupportedScheme(String),
    #[error("URLs with a user name or password are not allowed")]
    CredentialsInUrl,
    #[error("Address {0} is local or private and cannot be downloaded from")]
    ForbiddenAddress(IpAddr),
    #[error("Local host names are not allowed")]
    ForbiddenHost,
    #[error("Could not resolve host {host}: {reason}")]
    DnsFailure { host: String, reason: String },
    #[error("Too many redirects")]
    TooManyRedirects,
    #[error("The server sent an invalid redirect")]
    BadRedirect,
    #[error("The server answered with HTTP status {0}")]
    Http(u16),
    #[error("The file is larger than the {limit_mib} MiB limit")]
    TooLarge { limit_mib: u64 },
    #[error("The download timed out")]
    Timeout,
    #[error("The download was cancelled")]
    Cancelled,
    #[error("This link points to a web page, not directly to an audio file")]
    HtmlPage,
    #[error("The downloaded data is not a supported audio file (WAV or MP3)")]
    NotAudio,
    #[error("Network error: {0}")]
    Network(String),
    #[error("File error: {0}")]
    Io(String),
}
