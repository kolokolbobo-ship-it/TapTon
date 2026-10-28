use std::ffi::OsString;
use taptone::errors::ProtocolError;
use taptone::protocol::{find_uri_arg, has_supported_scheme, parse_uri, ProtocolCommand};

fn play_url(uri: &str) -> String {
    match parse_uri(uri).unwrap_or_else(|e| panic!("{uri} should parse: {e:?}")) {
        ProtocolCommand::Play { url } => url.to_string(),
    }
}

// ---- The seven links from the task description ----------------------------------------------

#[test]
fn required_link_1_plain_https() {
    assert_eq!(
        play_url("soundpad://sound/url/https://cdn.example.com/a.mp3"),
        "https://cdn.example.com/a.mp3"
    );
}

#[test]
fn required_link_2_uppercase_scheme() {
    assert_eq!(
        play_url("soundpad://sound/url/HTTPS://cdn.example.com/a.mp3"),
        "https://cdn.example.com/a.mp3"
    );
}

#[test]
fn required_link_3_fully_encoded() {
    assert_eq!(
        play_url("soundpad://sound/url/https%3A%2F%2Fcdn.example.com%2Fa.mp3"),
        "https://cdn.example.com/a.mp3"
    );
}

#[test]
fn required_link_4_collapsed_slash_and_trailing_slash() {
    assert_eq!(
        play_url("soundpad://sound/url/https:/cdn.example.com/a.mp3/"),
        "https://cdn.example.com/a.mp3"
    );
}

#[test]
fn required_link_5_query_parameter() {
    assert_eq!(
        play_url("soundpad://play?url=https%3A%2F%2Fcdn.example.com%2Fa.mp3"),
        "https://cdn.example.com/a.mp3"
    );
}

#[test]
fn required_link_6_nested_url_keeps_the_earliest_scheme() {
    assert_eq!(
        play_url("soundpad://x/https://a.com/?r=http://b.com"),
        "https://a.com/?r=http://b.com"
    );
}

#[test]
fn required_link_7_taptone_scheme() {
    assert_eq!(
        play_url("taptone://sound/url/https://cdn.example.com/a.mp3"),
        "https://cdn.example.com/a.mp3"
    );
}

// ---- http / https detection ------------------------------------------------------------------

#[test]
fn earliest_of_http_and_https_wins_in_both_orders() {
    assert_eq!(
        play_url("soundpad://sound/url/https://example.com/audio.mp3?redirect=http://other.com/audio.mp3"),
        "https://example.com/audio.mp3?redirect=http://other.com/audio.mp3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/http://example.com/audio.mp3?redirect=https://other.com/audio.mp3"),
        "http://example.com/audio.mp3?redirect=https://other.com/audio.mp3"
    );
}

#[test]
fn scheme_is_case_insensitive_but_path_keeps_its_case() {
    for scheme in ["https", "HTTPS", "Https", "HtTpS"] {
        assert_eq!(
            play_url(&format!(
                "soundpad://sound/url/{scheme}://Example.com/Path/Sound.MP3?Q=AbC"
            )),
            "https://example.com/Path/Sound.MP3?Q=AbC"
        );
    }
    assert_eq!(
        play_url("soundpad://sound/url/Http://example.com/audio.mp3"),
        "http://example.com/audio.mp3"
    );
}

// ---- Percent-encoding ------------------------------------------------------------------------

#[test]
fn partially_encoded_links() {
    let want = "https://site.com/a.mp3";
    assert_eq!(play_url("soundpad://sound/url/https:%2F%2Fsite.com%2Fa.mp3"), want);
    assert_eq!(play_url("soundpad://sound/url/https%3A//site.com/a.mp3"), want);
    assert_eq!(play_url("soundpad://sound/url/https://site.com%2Fa.mp3"), want);
    assert_eq!(play_url("soundpad://sound/url/HTTPS%3a%2f%2fsite.com%2fa.mp3"), want);
}

#[test]
fn encoded_url_is_decoded_exactly_once() {
    assert_eq!(
        play_url("soundpad://sound/url/https%3A%2F%2Fsite.com%2Fa%2520b.mp3"),
        "https://site.com/a%20b.mp3"
    );
    assert_eq!(
        play_url("soundpad://play?url=https%3A%2F%2Fsite.com%2Fa%3Fx%3D1%26y%3D2"),
        "https://site.com/a?x=1&y=2"
    );
}

#[test]
fn plain_url_is_never_decoded() {
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/a%20b.mp3"),
        "https://site.com/a%20b.mp3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/a%2Fb.mp3"),
        "https://site.com/a%2Fb.mp3"
    );
}

#[test]
fn utf8_and_invalid_utf8_are_handled_safely() {
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/звук.mp3"),
        "https://site.com/%D0%B7%D0%B2%D1%83%D0%BA.mp3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https%3A%2F%2Fsite.com%2F%D0%B7.mp3"),
        "https://site.com/%D0%B7.mp3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https%3A%2F%2Fsite.com%2F%FF.mp3"),
        "https://site.com/%FF.mp3"
    );
}

#[test]
fn encoded_control_characters_are_rejected() {
    let r = parse_uri("soundpad://play?url=https%3A%2F%2Fsite.com%2F%0Aevil.mp3");
    assert!(matches!(r, Err(ProtocolError::InvalidTargetUrl(_))), "{r:?}");
}

// ---- Query parameter -------------------------------------------------------------------------

#[test]
fn url_parameter_variants() {
    assert_eq!(
        play_url("soundpad://play?url=https%3A%2F%2Fa.com%2Fb.mp3&volume=5"),
        "https://a.com/b.mp3"
    );
    assert_eq!(
        play_url("soundpad://play?volume=5&url=https%3A%2F%2Fa.com%2Fb.mp3"),
        "https://a.com/b.mp3"
    );
    assert_eq!(
        play_url("soundpad://play?URL=https://a.com/b.mp3&volume=5"),
        "https://a.com/b.mp3"
    );
    assert_eq!(
        play_url("soundpad://play?url=https://a.com/b.mp3?t=1&u=2"),
        "https://a.com/b.mp3?t=1&u=2"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https://a.com/b.mp3?url=https://c.com/d.mp3"),
        "https://a.com/b.mp3?url=https://c.com/d.mp3"
    );
}

// ---- Slashes ---------------------------------------------------------------------------------

#[test]
fn collapsed_slashes_are_repaired_only_after_the_scheme() {
    assert_eq!(
        play_url("soundpad://sound/url/https:/cdn.example.com/dir//a.mp3"),
        "https://cdn.example.com/dir//a.mp3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/http:/cdn.example.com/a.mp3"),
        "http://cdn.example.com/a.mp3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https:///cdn.example.com/a.mp3"),
        "https://cdn.example.com/a.mp3"
    );
}

#[test]
fn trailing_slash_is_removed_only_when_it_is_clearly_spurious() {
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/a.MP3/"),
        "https://site.com/a.MP3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/a.mp3//"),
        "https://site.com/a.mp3"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/audio/"),
        "https://site.com/audio/"
    );
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/v1.2/"),
        "https://site.com/v1.2/"
    );
    assert_eq!(play_url("soundpad://sound/url/https://site.com/"), "https://site.com/");
    assert_eq!(
        play_url("soundpad://sound/url/https://site.com/a.mp3?x=1/"),
        "https://site.com/a.mp3?x=1/"
    );
}

// ---- Quotes, spaces, no slashes --------------------------------------------------------------

#[test]
fn quotes_spaces_and_missing_slashes() {
    assert_eq!(
        play_url("\"soundpad://sound/url/https://site.com/my sound.mp3\""),
        "https://site.com/my%20sound.mp3"
    );
    assert_eq!(
        play_url("  'taptone://sound/url/https://a.com/a.mp3'  "),
        "https://a.com/a.mp3"
    );
    assert_eq!(
        play_url("soundpad:play?url=https%3A%2F%2Fa.example%2Fx.wav"),
        "https://a.example/x.wav"
    );
    assert_eq!(
        play_url("taptone://PLAY?url=https%3A%2F%2Fa.example%2Fx.wav"),
        "https://a.example/x.wav"
    );
    assert_eq!(
        play_url("SOUNDPAD://play?url=https%3A%2F%2Fa.example%2Fx.wav"),
        "https://a.example/x.wav"
    );
}

// ---- Errors ----------------------------------------------------------------------------------

#[test]
fn rejects_missing_url() {
    assert_eq!(parse_uri("soundpad://play").unwrap_err(), ProtocolError::MissingUrl);
    assert_eq!(
        parse_uri("soundpad://play?url=").unwrap_err(),
        ProtocolError::MissingUrl
    );
    assert_eq!(
        parse_uri("soundpad://play?other=1").unwrap_err(),
        ProtocolError::MissingUrl
    );
    assert_eq!(
        parse_uri("soundpad://sound/url/").unwrap_err(),
        ProtocolError::MissingUrl
    );
    assert_eq!(
        parse_uri("soundpad://sound/url/ftp://a.com/x.mp3").unwrap_err(),
        ProtocolError::MissingUrl
    );
    assert_eq!(
        parse_uri("soundpad://sound/play/5").unwrap_err(),
        ProtocolError::MissingUrl
    );
}

#[test]
fn rejects_unsupported_command_and_scheme() {
    assert!(matches!(
        parse_uri("soundpad://delete?url=https%3A%2F%2Fa.example%2Fx.wav"),
        Err(ProtocolError::UnsupportedCommand(_))
    ));
    assert!(matches!(
        parse_uri("http://example.com/a.mp3"),
        Err(ProtocolError::UnsupportedScheme(_))
    ));
    assert!(matches!(
        parse_uri("file:///C:/x.wav"),
        Err(ProtocolError::UnsupportedScheme(_))
    ));
}

#[test]
fn rejects_garbage_without_panicking() {
    assert!(matches!(parse_uri("not a uri"), Err(ProtocolError::NotAUri(_))));
    assert!(matches!(parse_uri(""), Err(ProtocolError::NotAUri(_))));
    assert!(matches!(parse_uri("   "), Err(ProtocolError::NotAUri(_))));
    assert!(matches!(
        parse_uri("soundpad://sound/url/https://a.com/\u{0007}x.mp3"),
        Err(ProtocolError::NotAUri(_))
    ));
    let long = format!("soundpad://play?url={}", "a".repeat(9000));
    assert!(matches!(parse_uri(&long), Err(ProtocolError::NotAUri(_))));
    let long_target = format!("soundpad://sound/url/https://a.com/{}.mp3", "a".repeat(9000));
    assert!(matches!(parse_uri(&long_target), Err(ProtocolError::NotAUri(_))));
}

#[test]
fn rejects_bad_targets() {
    assert!(matches!(
        parse_uri("soundpad://play?url=%ZZ"),
        Err(ProtocolError::InvalidTargetUrl(_))
    ));
    assert!(matches!(
        parse_uri("soundpad://sound/url/https://"),
        Err(ProtocolError::InvalidTargetUrl(_))
    ));
    for target in [
        "file%3A%2F%2F%2FC%3A%2Fa.mp3",
        "javascript%3Aalert(1)",
        "data%3Aaudio%2Fwav%3Bbase64%2CAAAA",
    ] {
        let r = parse_uri(&format!("soundpad://play?url={target}"));
        assert!(
            matches!(r, Err(ProtocolError::InvalidTargetUrl(_))),
            "{target} -> {r:?}"
        );
    }
}

#[test]
fn http_targets_are_parsed_and_rejected_later_by_the_downloader() {
    let ProtocolCommand::Play { url } = parse_uri("soundpad://play?url=http%3A%2F%2Fexample.com%2Fa.mp3").unwrap();
    assert_eq!(url.scheme(), "http");
    assert!(taptone::downloader::validate_url(&url).is_err());
}

// ---- Command line ----------------------------------------------------------------------------

#[test]
fn finds_uri_argument() {
    let args = vec![OsString::from("--flag"), OsString::from("soundpad://play?url=x")];
    assert_eq!(find_uri_arg(args).as_deref(), Some("soundpad://play?url=x"));
    assert_eq!(find_uri_arg(vec![OsString::from("--help")]), None);
    assert_eq!(find_uri_arg(Vec::<OsString>::new()), None);
}

#[test]
fn rejoins_a_link_split_by_unquoted_spaces() {
    let args = ["soundpad://sound/url/https://a.com/my", "sound.mp3", "--minimized"].map(OsString::from);
    let link = find_uri_arg(args).unwrap();
    assert_eq!(link, "soundpad://sound/url/https://a.com/my sound.mp3");
    assert_eq!(play_url(&link), "https://a.com/my%20sound.mp3");
}

#[test]
fn scheme_detection_for_ipc() {
    assert!(has_supported_scheme("soundpad://play?url=x"));
    assert!(has_supported_scheme("  \"TAPTONE://show\" "));
    assert!(!has_supported_scheme("GET / HTTP/1.1"));
    assert!(!has_supported_scheme(""));
}
