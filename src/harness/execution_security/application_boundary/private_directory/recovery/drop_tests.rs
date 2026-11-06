use super::*;
use std::{sync::mpsc, time::Duration};

/// Production boundary: an active provider context must retain its lease and
/// directory while another creator/reclaimer holds the shared coordinator.
/// Pure validators cannot prove Drop/creation ordering. Local temporary files
/// and one thread only; no provider, Docker or network; low setup cost.
#[test]
fn session_drop_waits_for_the_recovery_coordinator() {
    let root = tempfile::tempdir().unwrap();
    let session = PrivateDirectory::create_session_at(root.path()).unwrap();
    let path = session.path.clone();
    let coordinator =
        open_existing_lock(&root.path().join(SESSION_ROOT).join(COORDINATOR_LOCK)).unwrap();
    FileExt::lock_exclusive(&coordinator).unwrap();
    let (started_tx, started) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        drop(session);
        done_tx.send(()).unwrap();
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        done.recv_timeout(Duration::from_millis(50)).is_err(),
        "session disposal must wait for the creator/reclaimer"
    );
    assert!(path.is_dir());
    let active = open_existing_lock(&path.join(ACTIVE_LOCK)).unwrap();
    assert_eq!(
        FileExt::try_lock_exclusive(&active).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(active);
    FileExt::unlock(&coordinator).unwrap();
    done.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.join().unwrap();
    assert!(!path.exists());
    let next = PrivateDirectory::create_session_at(root.path()).unwrap();
    assert!(next.path.is_dir());
}
