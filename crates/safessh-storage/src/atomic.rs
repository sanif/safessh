//! Atomic file writes via tempfile + rename.

use std::path::Path;
use tempfile::NamedTempFile;

// SAFETY-INVARIANT-5: All filesystem writes that could be observed by another
// process MUST go through these functions. tempfile + rename is the only path
// that guarantees no half-written file is ever observable.
pub fn write_string(path: &Path, contents: &str) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent dir"))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    use std::io::Write;
    tmp.write_all(contents.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

// SAFETY-INVARIANT-5: see `write_string` above.
pub fn write_bytes(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent dir"))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    use std::io::Write;
    tmp.write_all(contents)?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
