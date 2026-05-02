use safessh_ssh::driver::SshDriver;
use safessh_ssh::mock::MockSshDriver;
use safessh_storage::project::Target;

fn target() -> Target {
    Target::Inline {
        name: "t".into(),
        host: "h".into(),
        port: 22,
        user: "u".into(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    }
}

#[tokio::test]
async fn read_file_truncates_at_cap() {
    let driver = MockSshDriver::new();
    driver.put_file("t", "/etc/hostname", b"x".repeat(100));
    let result = driver
        .read_file(&target(), "/etc/hostname", 10)
        .await
        .unwrap();
    assert_eq!(result.bytes.len(), 10);
    assert!(result.truncated);
}

#[tokio::test]
async fn read_file_missing() {
    let driver = MockSshDriver::new();
    let err = driver
        .read_file(&target(), "/no/such", 1024)
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("NotFound") || format!("{err}").contains("no such"));
}

#[tokio::test]
async fn write_then_read_round_trips() {
    let driver = MockSshDriver::new();
    let result = driver
        .write_file(&target(), "/tmp/x", b"hello")
        .await
        .unwrap();
    assert_eq!(result.bytes_written, 5);
    let r = driver.read_file(&target(), "/tmp/x", 1024).await.unwrap();
    assert_eq!(r.bytes, b"hello");
    assert!(!r.truncated);
}
