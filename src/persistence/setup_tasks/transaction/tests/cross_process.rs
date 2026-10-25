use super::*;
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Production boundary: concurrent setup scans must serialize their durable
/// read-modify-write across processes. A thread-only test cannot exercise the
/// OS lock because the process mutex serializes first. Uses a temporary state
/// directory and subprocesses; no Docker or network; filesystem locks are the
/// only external dependency and setup cost is low.
/// Production boundary: a slower scan from another process must not replace
/// a newer scan even when it finishes later. A unit test alone cannot verify
/// the child reloads the locked durable store; uses a subprocess and local
/// filesystem only (low cost).
#[test]
fn older_start_scan_finishing_later_cannot_replace_store_from_another_process() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let mut newer = snapshot(25, "newer observation");
    newer.scan_started_at_unix_ms = 20;
    reconcile_at(&path, &newer).unwrap();

    let completion_marker = directory.path().join("child-reconciled");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("persistence::setup_tasks::transaction::tests::cross_process::cross_process_lock_helper")
        .arg("--nocapture")
        .env("KOOLADE_SETUP_STATE_PATH", &path)
        .env("KOOLADE_SETUP_CHILD_AT", "30")
        .env("KOOLADE_SETUP_CHILD_START_AT", "10")
        .env("KOOLADE_SETUP_CHILD_TITLE", "stale child observation")
        .env("KOOLADE_SETUP_EXPECT_STALE", "1")
        .env("KOOLADE_SETUP_LOCK_COMPLETION_MARKER", &completion_marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let completed = wait_for_marker(&mut child, &completion_marker, Duration::from_secs(5));
    if !completed {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();

    assert!(completed, "child did not report its stale scan result");
    assert!(status.success(), "child test process failed: {status}");
    let store = load_from(&path).unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].title,
        "newer observation"
    );
    assert!(!store.tasks.contains_key("setup:docker:start"));
}

#[test]
fn cross_process_reconciliation_waits_for_the_os_file_lock() {
    use fs2::FileExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let lock_path = directory.path().join(LOCK_FILE);
    let attempt_marker = directory.path().join("child-attempted-lock");
    let completion_marker = directory.path().join("child-reconciled");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("persistence::setup_tasks::transaction::tests::cross_process::cross_process_lock_helper")
        .arg("--nocapture")
        .env("KOOLADE_SETUP_STATE_PATH", &path)
        .env("KOOLADE_SETUP_LOCK_ATTEMPT_MARKER", &attempt_marker)
        .env("KOOLADE_SETUP_LOCK_COMPLETION_MARKER", &completion_marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let reached_lock = wait_for_marker(&mut child, &attempt_marker, Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(100));
    let blocked_while_parent_holds_lock = !completion_marker.exists();
    FileExt::unlock(&lock).unwrap();
    let completed = wait_for_marker(&mut child, &completion_marker, Duration::from_secs(5));
    if !completed {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();

    assert!(reached_lock, "child did not reach the OS lock boundary");
    assert!(
        blocked_while_parent_holds_lock,
        "child reconciled while another process held the OS lock"
    );
    assert!(
        completed,
        "child did not reconcile after the lock was released"
    );
    assert!(status.success(), "child test process failed: {status}");
    let store = load_from(&path).unwrap();
    assert_eq!(store.tasks["setup:docker:rootless"].title, "child process");
}

/// Production boundary: a canceled reconciliation waiting on the shared task
/// file lock must not persist after release.
/// A unit-only predicate test cannot prove lock ordering; this uses a child
/// process and temporary local files, with no Docker/network dependency (low
/// setup cost).
#[test]
fn cross_process_cancellation_skips_the_waiting_snapshot() {
    use fs2::FileExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let lock_path = directory.path().join(LOCK_FILE);
    let attempt_marker = directory.path().join("child-attempted-lock");
    let cancellation_marker = directory.path().join("refresh-requested");
    let completion_marker = directory.path().join("child-reconciled");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("persistence::setup_tasks::transaction::tests::cross_process::cross_process_lock_helper")
        .arg("--nocapture")
        .env("KOOLADE_SETUP_STATE_PATH", &path)
        .env("KOOLADE_SETUP_LOCK_ATTEMPT_MARKER", &attempt_marker)
        .env("KOOLADE_SETUP_CANCEL_MARKER", &cancellation_marker)
        .env("KOOLADE_SETUP_EXPECT_CANCELLED", "1")
        .env("KOOLADE_SETUP_LOCK_COMPLETION_MARKER", &completion_marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let reached_lock = wait_for_marker(&mut child, &attempt_marker, Duration::from_secs(5));
    std::fs::write(&cancellation_marker, b"refresh").unwrap();
    FileExt::unlock(&lock).unwrap();
    let completed = wait_for_marker(&mut child, &completion_marker, Duration::from_secs(5));
    if !completed {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();

    assert!(reached_lock, "child did not reach the OS lock boundary");
    assert!(completed, "child did not report the canceled scan");
    assert!(status.success(), "child test process failed: {status}");
    let store = load_from(&path).unwrap();
    assert!(!store.tasks.contains_key("setup:docker:rootless"));
    assert_eq!(store.last_scan_started_at_unix_ms, 0);
}

#[test]
fn cross_process_lock_helper() {
    let Some(path) = std::env::var_os("KOOLADE_SETUP_STATE_PATH") else {
        return;
    };
    let path = Path::new(&path);
    let at = std::env::var("KOOLADE_SETUP_CHILD_AT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(30);
    let title =
        std::env::var("KOOLADE_SETUP_CHILD_TITLE").unwrap_or_else(|_| "child process".into());
    let mut snapshot = snapshot(at, &title);
    snapshot.scan_started_at_unix_ms = std::env::var("KOOLADE_SETUP_CHILD_START_AT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(at);
    let cancel_marker = std::env::var_os("KOOLADE_SETUP_CANCEL_MARKER");
    let result = if let Some(marker) = cancel_marker {
        let marker = Path::new(&marker).to_path_buf();
        reconcile_at_if(path, &snapshot, || !marker.exists())
    } else {
        reconcile_at(path, &snapshot).map(Some)
    };
    if std::env::var_os("KOOLADE_SETUP_EXPECT_STALE").is_some() {
        assert!(result.is_err(), "same-time child observation was accepted");
    } else if std::env::var_os("KOOLADE_SETUP_EXPECT_CANCELLED").is_some() {
        assert!(matches!(result, Ok(None)), "canceled child scan was saved");
    } else {
        result.unwrap().unwrap();
    }
    fs::write(
        std::env::var_os("KOOLADE_SETUP_LOCK_COMPLETION_MARKER").unwrap(),
        b"complete",
    )
    .unwrap();
}

fn wait_for_marker(child: &mut Child, path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}
