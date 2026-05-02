//! Library target for `safessh-cli`.
//!
//! Exposes internal modules so integration tests can inject mock drivers
//! without going through the binary.  Nothing here is part of the public
//! API of the safessh workspace — it is `pub` only for `#[cfg(test)]`
//! and the crate-level integration tests in `tests/`.

pub mod commands;
pub mod errors;
pub mod output;
pub mod prompt;
pub mod cli;
