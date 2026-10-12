//! Keep immediate FD-release assertions separate from subprocess-spawning tests.
//! A concurrent fork can inherit an unrelated test's open-file description and
//! keep its flock alive after the parent drops it. Do not fork or spawn here.

use herdr_lineup::lock::{RuntimePaths, SessionLock};

#[test]
fn independent_open_cannot_claim_a_live_lock_and_stale_pid_does_not_matter() {
    let dir = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::new(dir.path(), std::path::Path::new("/fake/socket")).unwrap();
    let first = SessionLock::acquire(&paths).unwrap().unwrap();
    std::fs::write(&paths.pid, "1").unwrap();
    assert!(SessionLock::acquire(&paths).unwrap().is_none());
    drop(first);
    assert!(SessionLock::acquire(&paths).unwrap().is_some());
}
