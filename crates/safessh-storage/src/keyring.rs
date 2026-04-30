//! Keychain abstraction.
//!
//! `KeychainProvider` is a trait so callers can depend on the abstraction and
//! tests can swap in `mock::MockKeychain`. `SystemKeychain` delegates to the
//! `keyring` crate using `"safessh"` as the service name.
//!
//! All errors from the underlying `keyring` crate are mapped to
//! [`safessh_core::error::Error::Storage`].

use safessh_core::error::{Error, Result};

/// Trait describing the small surface area we need from a keychain.
///
/// Implementations must be `Send + Sync` so a single instance can be shared
/// across threads (e.g. behind an `Arc`) by the CLI runtime.
pub trait KeychainProvider: Send + Sync {
    /// Fetch a secret by name. Returns `Error::Storage` if missing or on any
    /// underlying keychain failure.
    fn get(&self, secret_name: &str) -> Result<String>;

    /// Store a secret by name, overwriting any existing value.
    fn set(&self, secret_name: &str, value: &str) -> Result<()>;

    /// Remove a secret by name. Implementations may return `Error::Storage`
    /// if the secret does not exist, depending on the underlying backend.
    fn delete(&self, secret_name: &str) -> Result<()>;
}

/// Production keychain backed by the OS keyring (Keychain on macOS,
/// Credential Manager on Windows, Secret Service on Linux).
pub struct SystemKeychain;

impl KeychainProvider for SystemKeychain {
    fn get(&self, secret_name: &str) -> Result<String> {
        keyring::Entry::new("safessh", secret_name)
            .and_then(|e| e.get_password())
            .map_err(|e| Error::Storage(format!("keyring get: {e}")))
    }

    fn set(&self, secret_name: &str, value: &str) -> Result<()> {
        keyring::Entry::new("safessh", secret_name)
            .and_then(|e| e.set_password(value))
            .map_err(|e| Error::Storage(format!("keyring set: {e}")))
    }

    fn delete(&self, secret_name: &str) -> Result<()> {
        keyring::Entry::new("safessh", secret_name)
            .and_then(|e| e.delete_password())
            .map_err(|e| Error::Storage(format!("keyring delete: {e}")))
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod mock {
    //! In-memory keychain for tests.
    //!
    //! Available under `#[cfg(test)]` always, and to dependent crates that
    //! enable the `test-support` feature.
    use super::{Error, KeychainProvider, Result};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MockKeychain {
        inner: Mutex<HashMap<String, String>>,
    }

    impl KeychainProvider for MockKeychain {
        fn get(&self, k: &str) -> Result<String> {
            self.inner
                .lock()
                .unwrap()
                .get(k)
                .cloned()
                .ok_or_else(|| Error::Storage(format!("not found: {k}")))
        }

        fn set(&self, k: &str, v: &str) -> Result<()> {
            self.inner.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }

        fn delete(&self, k: &str) -> Result<()> {
            self.inner.lock().unwrap().remove(k);
            Ok(())
        }
    }
}
