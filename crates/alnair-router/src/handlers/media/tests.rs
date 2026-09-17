use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::fetch::{is_private_ip, validate_fetch_url};
use super::multipart::{multipart_field, rewrite_multipart_field};

/// A multipart body with a text field followed by a file part.
const MULTIPART: &str = "----boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nopenai/whisper-1\r\n------boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\n\x00\x01\x02\r\n------boundary--\r\n";

#[test]
fn multipart_field_reads_a_text_part() {
    let value = multipart_field(
        MULTIPART.as_bytes(),
        Some("multipart/form-data; boundary=----boundary"),
        "model",
    );
    assert_eq!(value.as_deref(), Some("openai/whisper-1"));
}

#[test]
fn multipart_field_ignores_missing_fields_and_other_content_types() {
    let value = multipart_field(
        MULTIPART.as_bytes(),
        Some("multipart/form-data"),
        "language",
    );
    assert_eq!(value, None);

    let json = br#"{"model":"openai/whisper-1"}"#;
    assert_eq!(
        multipart_field(json, Some("application/json"), "model"),
        None
    );
    assert_eq!(multipart_field(MULTIPART.as_bytes(), None, "model"), None);
}

#[test]
fn multipart_field_does_not_confuse_a_filename_with_a_field_name() {
    // The file part's `filename="model"` must not be read as the field.
    let body = "----b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"model\"\r\n\r\nbytes\r\n------b--\r\n";
    let value = multipart_field(body.as_bytes(), Some("multipart/form-data"), "model");
    assert_eq!(value, None);
}

#[test]
fn rewrite_multipart_field_replaces_only_the_value() {
    let rewritten = rewrite_multipart_field(
        MULTIPART.as_bytes(),
        Some("multipart/form-data; boundary=----boundary"),
        "model",
        "whisper-1",
    );
    let text = String::from_utf8_lossy(&rewritten);

    assert!(text.contains("\r\n\r\nwhisper-1\r\n"));
    assert!(!text.contains("openai/whisper-1"));
    // Every other part survives untouched.
    assert!(text.contains("name=\"file\"; filename=\"a.mp3\""));
    assert!(text.contains("------boundary--"));
}

/// File parts are arbitrary bytes; a lossy UTF-8 round trip would shift the
/// offsets after them and corrupt the body.
#[test]
fn rewrite_multipart_field_survives_binary_file_bytes() {
    let mut body = Vec::new();
    body.extend_from_slice(b"----b\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nopenai/whisper-1\r\n------b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\n");
    let binary = [0xffu8, 0xfe, 0x00, 0x80, 0xc3, 0x28, 0x01];
    body.extend_from_slice(&binary);
    body.extend_from_slice(b"\r\n------b--\r\n");

    let rewritten = rewrite_multipart_field(
        &body,
        Some("multipart/form-data; boundary=----b"),
        "model",
        "whisper-1",
    );

    // The binary part is still byte-for-byte what the caller sent.
    let tail = &rewritten[rewritten
        .windows(binary.len())
        .position(|window| window == binary)
        .expect("binary payload")..];
    assert_eq!(&tail[..binary.len()], &binary);
    assert!(String::from_utf8_lossy(&rewritten).contains("whisper-1\r\n"));
}

#[test]
fn rewrite_multipart_field_leaves_non_multipart_bodies_alone() {
    let json = br#"{"model":"openai/whisper-1"}"#;
    let rewritten = rewrite_multipart_field(json, Some("application/json"), "model", "whisper-1");
    assert_eq!(rewritten.as_ref(), json as &[u8]);
}

fn ip(value: &str) -> IpAddr {
    value.parse().expect("valid ip")
}

#[test]
fn private_ip_ranges_are_rejected() {
    for address in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
        IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V4(Ipv4Addr::BROADCAST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        ip("fd00::1"),
        ip("fe80::1"),
        ip("::ffff:127.0.0.1"),
    ] {
        assert!(is_private_ip(address), "{address} should be private");
    }
}

#[test]
fn public_ips_are_allowed() {
    for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
        assert!(
            !is_private_ip(ip(address)),
            "{address} should be considered public"
        );
    }
}

#[tokio::test]
async fn validate_fetch_url_rejects_private_targets_and_schemes() {
    for url in [
        "file:///etc/passwd",
        "http://localhost/admin",
        "http://127.0.0.1:8080/",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
    ] {
        let parsed = url::Url::parse(url).expect("valid url");
        assert!(
            validate_fetch_url(&parsed).await.is_err(),
            "{url} should be rejected"
        );
    }
}

#[tokio::test]
async fn validate_fetch_url_allows_public_literal_ips() {
    for url in ["https://1.1.1.1/", "https://[2606:4700:4700::1111]/"] {
        let parsed = url::Url::parse(url).expect("valid url");
        let resolved = validate_fetch_url(&parsed).await.expect("public ip");
        assert!(!resolved.is_empty(), "{url} should resolve to itself");
    }
}
