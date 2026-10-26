use super::*;
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Production boundary: a scan that reserved an older generation cannot
/// overwrite a newer durable result after finishing later. Uses a child process
/// and local filesystem only; no Docker or network, low setup cost.
#[test]
fn older_reserved_generation_cannot_replace_a_newer_child_result() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let slow_generation = begin_scan_at(&path).unwrap();
    let completion_marker = directory.path().join("child-reconciled");
    let mut child = helper(&path, &completion_marker)
        .env("KOOLADE_SETUP_CHILD_TITLE", "newer child scan")
        .spawn()
        .unwrap();
    let completed = wait_for_marker(&mut child, &completion_marker, Duration::from_secs(5));
    if !completed {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    assert!(completed, "child did not report its committed scan");
    assert!(status.success(), "child test process failed: {status}");

    let slow = snapshot(slow_generation, "older reserved scan");
    assert!(reconcile_live_at(&path, &slow, slow_generation).is_err());
    let store = load_from(&path).unwrap().0;
    assert_eq!(
        store.tasks["setup:docker:rootless"].title,
        "newer child scan"
    );
}

/// Production boundary: separate app processes must serialize the task-store
/// read-modify-write with the OS lock. A thread-only test cannot cover this
/// boundary; local filesystem only, no external services, low setup cost.
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

    let mut child = helper(&path, &completion_marker)
        .env("KOOLADE_SETUP_LOCK_ATTEMPT_MARKER", &attempt_marker)
        .spawn()
        .unwrap();
    let reached_lock = wait_for_marker(&mut child, &attempt_marker, Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(100));
    let blocked = !completion_marker.exists();
    FileExt::unlock(&lock).unwrap();
    let completed = wait_for_marker(&mut child, &completion_marker, Duration::from_secs(5));
    if !completed {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();

    assert!(reached_lock, "child did not reach the OS lock boundary");
    assert!(blocked, "child reconciled while the parent held the lock");
    assert!(completed, "child did not reconcile after lock release");
    assert!(status.success(), "child test process failed: {status}");
    let store = load_from(&path).unwrap().0;
    assert_eq!(store.tasks["setup:docker:rootless"].title, "child process");
}

/// Production boundary: cancellation while a reserved snapshot waits on the
/// shared OS file lock must skip it after release. A thread-only test cannot
/// prove this cross-process lock boundary. Local filesystem only; no Docker or
/// network, low setup cost.
#[test]
fn cross_process_cancellation_skips_the_waiting_snapshot() {
    use fs2::FileExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let lock_path = directory.path().join(LOCK_FILE);
    let attempt_marker = directory.path().join("child-attempted-lock");
    let cancellation_marker = directory.path().join("refresh-requested");
    let completion_marker = directory.path().join("child-reconciled");
    let reserved_generation = begin_scan_at(&path).unwrap();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let mut child = helper(&path, &completion_marker)
        .env("KOOLADE_SETUP_LOCK_ATTEMPT_MARKER", &attempt_marker)
        .env("KOOLADE_SETUP_CANCEL_MARKER", &cancellation_marker)
        .env(
            "KOOLADE_SETUP_RESERVED_GENERATION",
            reserved_generation.to_string(),
        )
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
    let store = load_from(&path).unwrap().0;
    assert!(store.tasks.is_empty());
    assert_eq!(store.latest_scan_generation, reserved_generation);
}

#[test]
fn cross_process_lock_helper() {
    let Some(path) = std::env::var_os("KOOLADE_SETUP_STATE_PATH") else {
        return;
    };
    let path = Path::new(&path);
    let title =
        std::env::var("KOOLADE_SETUP_CHILD_TITLE").unwrap_or_else(|_| "child process".into());
    let generation = match std::env::var("KOOLADE_SETUP_RESERVED_GENERATION") {
        Ok(generation) => generation.parse().unwrap(),
        Err(_) => begin_scan_at(path).unwrap(),
    };
    let scan = snapshot(generation, &title);
    let result = if let Some(marker) = std::env::var_os("KOOLADE_SETUP_CANCEL_MARKER") {
        let marker = Path::new(&marker).to_path_buf();
        reconcile_at_if(path, &scan, generation, || !marker.exists())
    } else {
        reconcile_live_at(path, &scan, generation).map(Some)
    };
    if std::env::var_os("KOOLADE_SETUP_CANCEL_MARKER").is_some() {
        assert!(matches!(result, Ok(None)), "canceled scan was persisted");
    } else {
        result.unwrap().unwrap();
    }
    fs::write(
        std::env::var_os("KOOLADE_SETUP_LOCK_COMPLETION_MARKER").unwrap(),
        b"complete",
    )
    .unwrap();
}

fn helper<'a>(path: &'a Path, completion: &'a Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg("persistence::setup_tasks::transaction::tests::cross_process::cross_process_lock_helper")
        .arg("--nocapture")
        .env("KOOLADE_SETUP_STATE_PATH", path)
        .env("KOOLADE_SETUP_LOCK_COMPLETION_MARKER", completion)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
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
