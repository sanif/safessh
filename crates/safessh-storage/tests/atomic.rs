use safessh_storage::atomic;
use safessh_storage::locking::LockedFile;

#[test]
fn atomic_write_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foo.toml");
    atomic::write_string(&path, "hello").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
}

#[test]
fn atomic_write_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foo.toml");
    atomic::write_string(&path, "first").unwrap();
    atomic::write_string(&path, "second").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
}

#[test]
fn locked_file_opens_and_drops() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lock");
    let _l1 = LockedFile::open_exclusive(&path).unwrap();
    drop(_l1);
    let _l2 = LockedFile::open_exclusive(&path).unwrap();
}
