//! safessh-core — shared types, errors, redactor.

pub mod error;
pub mod redactor;
pub mod tunnel;
pub mod types;

pub use error::{Error, Result};
