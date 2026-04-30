//! Integration test for the in-memory `MockKeychain`.
//!
//! Run with `cargo test --package safessh-storage --features test-support
//! --test keyring`. We deliberately do not exercise `SystemKeychain` from CI
//! because the OS keychain backends are not available in headless
//! environments.

use safessh_storage::keyring::{mock::MockKeychain, KeychainProvider};

#[test]
fn mock_keychain_round_trip() {
    let kc = MockKeychain::default();
    kc.set("prod-pass", "hunter2").unwrap();
    assert_eq!(kc.get("prod-pass").unwrap(), "hunter2");
    kc.delete("prod-pass").unwrap();
    assert!(kc.get("prod-pass").is_err());
}
