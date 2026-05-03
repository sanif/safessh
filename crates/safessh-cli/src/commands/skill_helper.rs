//! Thin wrappers around `safessh-skill` install internals used by
//! `skill update`'s in-place rewrite path.

use safessh_core::error::{Error, Result};
use std::path::Path;

pub fn write_section(path: &Path, body: &str) -> Result<()> {
    safessh_skill::sections::install_md_section(path, body)
}

pub fn write_file(path: &Path, body: &str) -> Result<()> {
    safessh_storage::atomic::write_string(path, body).map_err(Error::Io)
}
