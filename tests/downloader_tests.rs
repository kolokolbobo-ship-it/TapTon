use std::net::IpAddr;
use taptone::downloader::*;
use taptone::errors::DownloadError;
use url::Url;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn forbidden_ip_ranges() {
    for s in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "169.254.169.254",
        "0.0.0.0",
        "100.64.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "198.18.0.1",
        "240.0.0.1",
        "::1",
        "::",
        "fe80::1",
        "fc00::1",
        "fd12:3456::1",
        "ff02::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "64:ff9b::7f00:1",
        "2002:7f00:1::1",
        "2001:db8::1",
    ] {
        assert!(is_forbidden_ip(ip(s)), "{s} should be forbidden");
    }
}

#[test]
fn public_ips_are_allowed() {
    for s in [
        "8.8.8.8",
        "1.1.1.1",
        "93.184.216.34",
        "172.32.0.1",
        "2606:4700:4700::1111",
    ] {
        assert!(!is_forbidden_ip(ip(s)), "{s} should be allowed");
    }
}

#[test]
fn url_validation() {
    let ok = Url::parse("https://example.com/a.mp3").unwrap();
    assert!(validate_url(&ok).is_ok());
    for (u, check) in [
        ("http://example.com/a.mp3", 0),
        ("ftp://example.com/a.mp3", 0),
        ("https://user:pw@example.com/a.mp3", 1),
        ("https://127.0.0.1/a.mp3", 2),
        ("https://[::1]/a.mp3", 2),
        ("https://192.168.0.5/a.mp3", 2),
        ("https://localhost/a.mp3", 3),
        ("https://foo.localhost/a.mp3", 3),
        ("https://printer.local/a.mp3", 3),
    ] {
        let err = validate_url(&Url::parse(u).unwrap()).unwrap_err();
        let matches = match check {
            0 => matches!(err, DownloadError::UnsupportedScheme(_)),
            1 => matches!(err, DownloadError::CredentialsInUrl),
            2 => matches!(err, DownloadError::ForbiddenAddress(_)),
            _ => matches!(err, DownloadError::ForbiddenHost),
        };
        assert!(matches, "{u} -> {err:?}");
    }
}

#[test]
fn literal_private_hosts_fail_before_any_dns() {
    let u = Url::parse("https://10.0.0.7/x.wav").unwrap();
    assert!(matches!(resolve_checked(&u), Err(DownloadError::ForbiddenAddress(_))));
}

#[test]
fn size_limit_is_enforced() {
    assert!(enforce_limit(MAX_DOWNLOAD_BYTES, MAX_DOWNLOAD_BYTES).is_ok());
    assert!(matches!(
        enforce_limit(MAX_DOWNLOAD_BYTES + 1, MAX_DOWNLOAD_BYTES),
        Err(DownloadError::TooLarge { limit_mib: 50 })
    ));
}

#[test]
fn safe_filenames() {
    assert_eq!(safe_filename("../../etc/passwd"), "passwd");
    assert_eq!(safe_filename("C:\\Windows\\evil.exe"), "evil.exe");
    assert_eq!(safe_filename("a<b>c:d\"e|f?g*h.mp3"), "a_b_c_d_e_f_g_h.mp3");
    assert_eq!(safe_filename("  ..  "), "sound");
    assert_eq!(safe_filename(""), "sound");
    assert_eq!(safe_filename("CON.wav"), "_CON.wav");
    assert_eq!(safe_filename("lpt1"), "_lpt1");
    assert_eq!(safe_filename("com10.wav"), "com10.wav");
    assert!(safe_filename(&"x".repeat(500)).chars().count() <= 64);
}

#[test]
fn suggested_names_come_from_the_url_safely() {
    let u = Url::parse("https://example.com/sounds/Vine%20Boom.mp3?token=secret").unwrap();
    assert_eq!(suggested_name(&u), "Vine Boom");
    let u = Url::parse("https://example.com/").unwrap();
    assert_eq!(suggested_name(&u), "sound");
}

#[test]
fn redaction_removes_query_and_credentials() {
    let u = Url::parse("https://user:pw@example.com/a/b.mp3?token=secret#frag").unwrap();
    let r = redact_url(&u);
    assert_eq!(r, "https://example.com/a/b.mp3");
    assert!(!r.contains("secret") && !r.contains("pw"));
}

#[test]
fn cleanup_removes_only_part_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.part"), b"x").unwrap();
    std::fs::write(dir.path().join("keep.wav"), b"x").unwrap();
    assert_eq!(cleanup_partials(dir.path()), 1);
    assert!(dir.path().join("keep.wav").exists());
    assert!(!dir.path().join("a.part").exists());
}
