//! Round-trip tests for `MockDriver`.

use safessh_ssh::driver::{OutputChunk, SshDriver};
use safessh_ssh::mock::{CannedResponse, MockDriver};
use safessh_storage::project::Target;

fn inline_target(name: &str) -> Target {
    Target::Inline {
        name: name.to_string(),
        host: "h".to_string(),
        port: 22,
        user: "u".to_string(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    }
}

#[tokio::test]
async fn mock_returns_canned_response() {
    let mock = MockDriver::default();
    mock.with_response(
        "web",
        "ls",
        CannedResponse {
            stdout: b"foo\nbar\n".to_vec(),
            stderr: Vec::new(),
            exit: 0,
        },
    );

    let target = inline_target("web");
    let mut chunks: Vec<OutputChunk> = Vec::new();
    let result = mock
        .exec(
            &target,
            "ls",
            1024,
            1024,
            Box::new(|c| chunks.push(c)),
        )
        .await
        .expect("exec should succeed");

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout_bytes, 8);
    assert_eq!(result.stderr_bytes, 0);
    assert!(!result.truncated);

    assert_eq!(chunks.len(), 1);
    match &chunks[0] {
        OutputChunk::Stdout(b) => assert_eq!(b, b"foo\nbar\n"),
        OutputChunk::Stderr(_) => panic!("expected stdout chunk"),
    }
}

#[tokio::test]
async fn mock_streams_stdout_and_stderr() {
    let mock = MockDriver::new();
    mock.with_response(
        "db",
        "uptime",
        CannedResponse {
            stdout: b"up 7 days".to_vec(),
            stderr: b"warn: clock skew".to_vec(),
            exit: 2,
        },
    );

    let target = inline_target("db");
    let mut chunks: Vec<OutputChunk> = Vec::new();
    let result = mock
        .exec(
            &target,
            "uptime",
            1024,
            1024,
            Box::new(|c| chunks.push(c)),
        )
        .await
        .expect("exec should succeed");

    assert_eq!(result.exit_code, 2);
    assert_eq!(result.stdout_bytes, 9);
    assert_eq!(result.stderr_bytes, 16);
    assert_eq!(chunks.len(), 2);
    assert!(matches!(&chunks[0], OutputChunk::Stdout(b) if b == b"up 7 days"));
    assert!(matches!(&chunks[1], OutputChunk::Stderr(b) if b == b"warn: clock skew"));
}

#[tokio::test]
async fn mock_unmatched_call_returns_empty_success() {
    let mock = MockDriver::default();
    let target = inline_target("none");
    let mut chunks: Vec<OutputChunk> = Vec::new();
    let result = mock
        .exec(
            &target,
            "whoami",
            1024,
            1024,
            Box::new(|c| chunks.push(c)),
        )
        .await
        .expect("exec should succeed");

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout_bytes, 0);
    assert_eq!(result.stderr_bytes, 0);
    assert!(chunks.is_empty());
}
