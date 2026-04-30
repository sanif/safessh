use proptest::prelude::*;
use safessh_core::redactor::{RedactionType, Redactor};

#[test]
fn redacts_aws_access_key() {
    let r = Redactor::default();
    let input = b"AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\n";
    let (out, counts) = r.redact(input);
    let out_str = String::from_utf8_lossy(&out);
    assert!(!out_str.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(out_str.contains("<REDACTED:aws_access_key>"));
    assert_eq!(counts.get(&RedactionType::AwsAccessKey).copied(), Some(1));
}

#[test]
fn redacts_bearer_token() {
    let r = Redactor::default();
    let input = b"Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.foo.bar\n";
    let (out, _) = r.redact(input);
    assert!(!String::from_utf8_lossy(&out).contains("eyJhbGciOiJIUzI1NiJ9"));
}

#[test]
fn redacts_private_key_block() {
    let r = Redactor::default();
    let input =
        b"-----BEGIN OPENSSH PRIVATE KEY-----\nbase64==\n-----END OPENSSH PRIVATE KEY-----\n";
    let (out, _) = r.redact(input);
    assert!(!String::from_utf8_lossy(&out).contains("base64=="));
    assert!(String::from_utf8_lossy(&out).contains("<REDACTED:private_key_block>"));
}

#[test]
fn redacts_password_query() {
    let r = Redactor::default();
    let input = b"https://example.com?password=hunter2&user=alice";
    let (out, _) = r.redact(input);
    assert!(!String::from_utf8_lossy(&out).contains("hunter2"));
}

#[test]
fn no_redactions_for_clean_text() {
    let r = Redactor::default();
    let input = b"hello world\nthe sky is blue\n";
    let (out, counts) = r.redact(input);
    assert_eq!(out, input);
    assert!(counts.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn aws_key_never_leaks(prefix in "[a-z ]{0,30}", suffix in "[a-z ]{0,30}") {
        let token = "AKIAIOSFODNN7EXAMPLE";
        let input = format!("{prefix}{token}{suffix}");
        let (out, _) = Redactor::default().redact(input.as_bytes());
        let out_str = String::from_utf8_lossy(&out);
        prop_assert!(!out_str.contains(token));
    }
}
