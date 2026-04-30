//! Advisory exclusive file locks via `fs2`.

use std::fs::File;
use std::path::Path;

// SAFETY-INVARIANT-12: Concurrent CLI/TUI invocations must not corrupt each
// other's writes. All rule-file writes go through `LockedFile`, which holds
// an exclusive advisory lock for the lifetime of the value.
pub struct LockedFile(File);

impl LockedFile {
    /// Open the file for read/write, creating it if missing, then acquire an
    /// exclusive advisory lock. Blocks until the lock is acquired.
    pub fn open_exclusive(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let f = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        // Fully-qualified to avoid `unstable_name_collisions` with
        // upcoming std `File::lock_exclusive`.
        fs2::FileExt::lock_exclusive(&f)?;
        Ok(Self(f))
    }

    /// Consume the lock guard and return the underlying file. The advisory
    /// lock is released when the returned `File` is dropped (closing the fd
    /// releases the OS lock).
    pub fn into_inner(self) -> File {
        // We need to move out of `self.0` while suppressing our `Drop` impl
        // (which would unlock the file). Wrapping in `ManuallyDrop` prevents
        // our destructor from running so the lock is released only when the
        // returned `File` is itself dropped.
        let me = std::mem::ManuallyDrop::new(self);
        // SAFETY: `me` is a `ManuallyDrop<LockedFile>`, so its `Drop` impl
        // will not run. We bitwise-copy the inner `File` out; the original
        // is treated as moved.
        unsafe { std::ptr::read(&me.0) }
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        // Use fully-qualified path to silence the `unstable_name_collisions`
        // warning emitted because `File::unlock` is being added to std.
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
