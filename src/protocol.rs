//! Parsing of `soundpad://...` (and `taptone://...`) links.
//! Parsing is pure: it never touches the network, the registry or the OS.
//!
//! Supported shapes (the target must be an `http://` or `https://` URL):
//!
//! * `soundpad://sound/url/https://host/a.mp3`            (URL inside the path)
//! * `soundpad://sound/url/https%3A%2F%2Fhost%2Fa.mp3`    (fully or partly percent-encoded)
//! * `soundpad://sound/url/https:/host/a.mp3/`            (slashes collapsed, spurious trailing slash)
//! * `soundpad://play?url=https%3A%2F%2Fhost%2Fa.mp3`     (URL in the `url` parameter)
//!
//! Rules that keep the target URL intact:
//!
//! * the EARLIEST `http(s)` occurrence wins, so `.../https://a.com/?r=http://b.com` stays whole;
//! * the scheme is matched case-insensitively, the rest of the URL keeps its case;
//! * percent-decoding happens at most once, and only when the link itself is encoded
//!   (encoded scheme separator, or encoded `/` in the host part). A plain URL such as
//!   `https://a.com/a%20b.mp3` is never decoded. Only ASCII escapes are decoded, so bytes that are
//!   not valid UTF-8 can never be corrupted: they stay `%XX`;
//! * collapsed slashes are repaired only directly after the scheme;
//! * a trailing `/` is dropped only when the URL has no query/fragment and the last path segment
//!   has an audio file extension (`.../a.mp3/`), never for directories like `.../audio/`.

use crate::errors::ProtocolError;
use std::ffi::OsString;
use url::Url;

pub const SCHEMES: [&str; 2] = ["soundpad", "taptone"];
pub const MAX_URI_LEN: usize = 8192;

/// Verbs that are explicitly not "play this URL" and must not be treated as such.
const REJECTED_COMMANDS: [&str; 5] = ["delete", "remove", "clear", "exit", "quit"];
/// Extensions for which a trailing `/` is considered spurious.
const AUDIO_EXTENSIONS: [&str; 12] = [
    "mp3", "wav", "ogg", "oga", "opus", "flac", "m4a", "aac", "wma", "webm", "aif", "aiff",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolCommand {
    Play { url: Url },
}

/// Strip whitespace and one layer of quotes that shells, browsers or `%1` expansion may add.
fn clean_input(input: &str) -> &str {
    input.trim().trim_matches(|c| c == '"' || c == '\'').trim()
}

fn is_wrapper(c: char) -> bool {
    c == '"' || c == '\'' || c == '<' || c == '>' || c.is_whitespace()
}

/// True if `input` (as passed on a command line or over IPC) starts with a supported scheme.
pub fn has_supported_scheme(input: &str) -> bool {
    let s = clean_input(input);
    match s.split_once(':') {
        Some((scheme, _)) => SCHEMES.iter().any(|x| scheme.eq_ignore_ascii_case(x)),
        None => false,
    }
}

/// Parse any protocol link and extract the targeted audio URL (http/https).
pub fn parse_uri(input: &str) -> Result<ProtocolCommand, ProtocolError> {
    let s = clean_input(input);
    if s.is_empty() {
        return Err(ProtocolError::NotAUri("the link is empty".to_string()));
    }
    if s.len() > MAX_URI_LEN {
        return Err(ProtocolError::NotAUri("the link is too long".to_string()));
    }
    if s.chars().any(|c| c.is_control()) {
        return Err(ProtocolError::NotAUri(
            "the link contains control characters".to_string(),
        ));
    }
    let Some((scheme, rest)) = s.split_once(':') else {
        return Err(ProtocolError::NotAUri(
            "a link such as soundpad://sound/url/https://... is expected".to_string(),
        ));
    };
    if !SCHEMES.iter().any(|x| scheme.eq_ignore_ascii_case(x)) {
        return Err(ProtocolError::UnsupportedScheme(scheme.to_string()));
    }
    let rest = rest.trim_start_matches('/');

    let verb: String = rest
        .chars()
        .take_while(|c| !matches!(c, '/' | '?' | '#'))
        .collect::<String>()
        .to_ascii_lowercase();
    if REJECTED_COMMANDS.contains(&verb.as_str()) {
        return Err(ProtocolError::UnsupportedCommand(verb));
    }

    let first_target = find_target(rest);

    // 1. `?url=...`, but only if the `?` belongs to the protocol link and not to the inner URL.
    if let Some(q) = rest.find('?') {
        if first_target.as_ref().is_none_or(|(i, _)| q < *i) {
            if let Some(value) = url_param(&rest[q + 1..]) {
                return from_param(&value);
            }
        }
    }

    // 2. URL inside the path (or any other place).
    match first_target {
        Some((i, prefix)) => Ok(ProtocolCommand::Play {
            url: build_target(&rest[i..], &prefix)?,
        }),
        None => Err(ProtocolError::MissingUrl),
    }
}

fn from_param(value: &str) -> Result<ProtocolCommand, ProtocolError> {
    let v = value.trim_matches(is_wrapper);
    if v.is_empty() {
        return Err(ProtocolError::MissingUrl);
    }
    match find_target(v) {
        Some((0, prefix)) => Ok(ProtocolCommand::Play {
            url: build_target(v, &prefix)?,
        }),
        _ => Err(ProtocolError::InvalidTargetUrl(shorten(v))),
    }
}

fn shorten(s: &str) -> String {
    s.chars().take(80).collect()
}

/// Value of the `url` query parameter. An unencoded inner URL that has its own `?query` owns the
/// remaining `&` parts; otherwise the first `&` ends the value.
fn url_param(query: &str) -> Option<String> {
    let mut pos = 0usize;
    for part in query.split('&') {
        let start = pos;
        pos += part.len() + 1;
        if let Some((key, value)) = part.split_once('=') {
            if key.trim().eq_ignore_ascii_case("url") {
                if value.contains('?') {
                    return Some(query[start + key.len() + 1..].to_string());
                }
                return Some(value.to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Prefix {
    /// Byte length of `scheme + separator + slashes`.
    len: usize,
    secure: bool,
    /// The separator or a slash was percent-encoded.
    encoded: bool,
}

/// Match `^https?(:|%3A)(/|%2F)+` ignoring ASCII case.
fn match_prefix(s: &str) -> Option<Prefix> {
    let b = s.as_bytes();
    if b.len() < 6 || !b[..4].eq_ignore_ascii_case(b"http") {
        return None;
    }
    let mut i = 4;
    let secure = b[i].eq_ignore_ascii_case(&b's');
    if secure {
        i += 1;
    }
    let mut encoded = false;
    if b.get(i) == Some(&b':') {
        i += 1;
    } else if i + 2 < b.len() && b[i] == b'%' && b[i + 1] == b'3' && b[i + 2].eq_ignore_ascii_case(&b'a') {
        i += 3;
        encoded = true;
    } else {
        return None;
    }
    let mut slashes = 0;
    loop {
        if b.get(i) == Some(&b'/') {
            i += 1;
            slashes += 1;
        } else if i + 2 < b.len() && b[i] == b'%' && b[i + 1] == b'2' && b[i + 2].eq_ignore_ascii_case(&b'f') {
            i += 3;
            slashes += 1;
            encoded = true;
        } else {
            break;
        }
    }
    if slashes == 0 {
        None
    } else {
        Some(Prefix {
            len: i,
            secure,
            encoded,
        })
    }
}

/// Earliest http(s) occurrence (not glued to a preceding letter or digit).
fn find_target(s: &str) -> Option<(usize, Prefix)> {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i] != b'h' && b[i] != b'H' {
            continue;
        }
        if i > 0 && b[i - 1].is_ascii_alphanumeric() {
            continue;
        }
        if let Some(p) = match_prefix(&s[i..]) {
            return Some((i, p));
        }
    }
    None
}

/// Decode `%XX` escapes that stand for ASCII characters, exactly once. Escapes >= 0x80 are kept
/// as they are, so the result is always valid UTF-8 and nothing is double-decoded.
fn decode_ascii_escapes(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hi = (b[i + 1] as char).to_digit(16);
            let lo = (b[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                let v = (h * 16 + l) as u8;
                if v < 0x80 {
                    out.push(v as char);
                } else {
                    out.push_str(&s[i..i + 3]);
                }
                i += 3;
                continue;
            }
        }
        match s[i..].chars().next() {
            Some(c) => {
                out.push(c);
                i += c.len_utf8();
            }
            None => break,
        }
    }
    out
}

/// True if the part before the first `/ ? #` contains an encoded delimiter, e.g. `host%2Fa.mp3`.
fn host_part_encoded(after_prefix: &str) -> bool {
    let end = after_prefix.find(['/', '?', '#']).unwrap_or(after_prefix.len());
    let host = after_prefix[..end].to_ascii_lowercase();
    host.contains("%2f") || host.contains("%3f") || host.contains("%23")
}

fn last_segment_is_audio(path: &str) -> bool {
    let segment = path.rsplit('/').next().unwrap_or("");
    match segment.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && AUDIO_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)),
        None => false,
    }
}

/// `https://h/a.mp3/` -> `https://h/a.mp3`. Directories, queries and fragments are left alone.
fn fix_trailing_slash(url: &str) -> String {
    if !url.ends_with('/') || url.contains('?') || url.contains('#') {
        return url.to_string();
    }
    let trimmed = url.trim_end_matches('/');
    let Some(after_scheme) = trimmed.split_once("://").map(|(_, r)| r) else {
        return url.to_string();
    };
    // Needs at least one path segment after the host.
    match after_scheme.split_once('/') {
        Some((_, path)) if last_segment_is_audio(path) => trimmed.to_string(),
        _ => url.to_string(),
    }
}

fn build_target(tail: &str, prefix: &Prefix) -> Result<Url, ProtocolError> {
    let tail = tail.trim_end_matches(is_wrapper);
    let after = tail.get(prefix.len..).unwrap_or("");
    let decoded = if prefix.encoded || host_part_encoded(after) {
        decode_ascii_escapes(tail)
    } else {
        tail.to_string()
    };

    let Some(p) = match_prefix(&decoded) else {
        return Err(ProtocolError::InvalidTargetUrl(shorten(&decoded)));
    };
    let scheme = if p.secure { "https" } else { "http" };
    let rebuilt = format!("{scheme}://{}", decoded[p.len..].trim_start_matches('/'));
    if rebuilt.chars().any(|c| c.is_control()) {
        return Err(ProtocolError::InvalidTargetUrl(
            "the URL contains control characters".to_string(),
        ));
    }
    let fixed = fix_trailing_slash(&rebuilt);
    let url = Url::parse(&fixed).map_err(|e| ProtocolError::InvalidTargetUrl(format!("{} ({e})", shorten(&fixed))))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(|h| h.is_empty()) {
        return Err(ProtocolError::InvalidTargetUrl(shorten(&fixed)));
    }
    Ok(url)
}

/// Interpret text typed or pasted by the user: a protocol link, or a plain / encoded http(s) URL
/// (with the same repairs as for links: collapsed slashes, a spurious trailing slash, ...).
pub fn parse_url_text(text: &str) -> Result<Url, ProtocolError> {
    let s = clean_input(text);
    if s.is_empty() {
        return Err(ProtocolError::MissingUrl);
    }
    if has_supported_scheme(s) {
        return match parse_uri(s)? {
            ProtocolCommand::Play { url } => Ok(url),
        };
    }
    if s.len() > MAX_URI_LEN {
        return Err(ProtocolError::NotAUri("the link is too long".to_string()));
    }
    match find_target(s) {
        Some((0, prefix)) => build_target(s, &prefix),
        _ => Err(ProtocolError::InvalidTargetUrl(shorten(s))),
    }
}

/// Find a protocol link among process arguments. If a link was split by unquoted spaces, the
/// following plain arguments (those not starting with `-`) are joined back with single spaces.
pub fn find_uri_arg<I: IntoIterator<Item = OsString>>(args: I) -> Option<String> {
    let args: Vec<String> = args.into_iter().filter_map(|a| a.into_string().ok()).collect();
    let idx = args.iter().position(|a| has_supported_scheme(a))?;
    let mut link = args[idx].clone();
    for extra in &args[idx + 1..] {
        if extra.starts_with('-') || has_supported_scheme(extra) {
            break;
        }
        link.push(' ');
        link.push_str(extra);
    }
    Some(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> String {
        match parse_uri(s).unwrap() {
            ProtocolCommand::Play { url } => url.to_string(),
        }
    }

    #[test]
    fn path_style() {
        assert_eq!(
            ok("soundpad://sound/url/https://cdn.x.com/a.mp3"),
            "https://cdn.x.com/a.mp3"
        );
    }
    #[test]
    fn query_style() {
        assert_eq!(
            ok("soundpad://play?url=https%3A%2F%2Fcdn.x.com%2Fa.mp3"),
            "https://cdn.x.com/a.mp3"
        );
    }
    #[test]
    fn fully_encoded() {
        assert_eq!(
            ok("soundpad://sound/url/https%3A%2F%2Fcdn.x.com%2Fa.mp3"),
            "https://cdn.x.com/a.mp3"
        );
    }
    #[test]
    fn collapsed_and_trailing_slash() {
        assert_eq!(
            ok("soundpad://sound/url/https:/cdn.x.com/a.mp3/"),
            "https://cdn.x.com/a.mp3"
        );
    }
    #[test]
    fn earliest_scheme_wins() {
        assert_eq!(
            ok("soundpad://x/https://a.com/?r=http://b.com"),
            "https://a.com/?r=http://b.com"
        );
        assert_eq!(
            ok("soundpad://x/http://a.com/a.mp3?r=https://b.com/b.mp3"),
            "http://a.com/a.mp3?r=https://b.com/b.mp3"
        );
    }
    #[test]
    fn directory_slash_is_kept() {
        assert_eq!(
            ok("soundpad://sound/url/https://site.com/audio/"),
            "https://site.com/audio/"
        );
    }
    #[test]
    fn pasted_text_gets_the_same_repairs() {
        let u = |t: &str| parse_url_text(t).unwrap().to_string();
        assert_eq!(u("https://a.com/x.mp3"), "https://a.com/x.mp3");
        assert_eq!(u("  \"HTTPS:/a.com/x.mp3/\"  "), "https://a.com/x.mp3");
        assert_eq!(u("https%3A%2F%2Fa.com%2Fx.mp3"), "https://a.com/x.mp3");
        assert_eq!(u("soundpad://sound/url/https://a.com/x.mp3"), "https://a.com/x.mp3");
        assert!(matches!(
            parse_url_text("ftp://a.com/x.mp3"),
            Err(ProtocolError::InvalidTargetUrl(_))
        ));
        assert_eq!(parse_url_text("  ").unwrap_err(), ProtocolError::MissingUrl);
    }

    #[test]
    fn decode_is_single_pass() {
        assert_eq!(decode_ascii_escapes("a%2520b%FF%zz%4"), "a%20b%FF%zz%4");
    }
}
