use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const CHILD_ROOT_ENV: &str = "KOOLADE_PRIVATE_SESSION_TEST_ROOT";
const CHILD_RELEASE: &str = "child.release";
const CHILD_ACTIVE_PATH: &str = "child.active-path";

/// Production boundary: startup recovery races with a provider process in a
/// different app process. A same-process unit test cannot prove the OS lock
/// keeps another process's private auth copy alive. Dependencies: one local
/// test child process and temporary files; no Docker, network, CLI, or user
/// credentials. Setup cost is low (one child launch, bounded to ten seconds).
#[test]
fn stale_session_recovery_preserves_live_runs_and_removes_crashed_auth_copies() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join(SESSION_ROOT);
    ensure_private_root(&sessions).unwrap();

    let stale = sessions.join(format!("run-{}-{}", u32::MAX, uuid::Uuid::new_v4()));
    super::super::create_private_directory(&stale).unwrap();
    let stale_lock = create_lock_file(&stale.join(ACTIVE_LOCK)).unwrap();
    drop(stale_lock);
    fs::create_dir_all(stale.join("codex-home")).unwrap();
    fs::write(stale.join("codex-home/auth.json"), b"synthetic-auth-copy").unwrap();

    let legacy_stale = root.path().join(format!(
        "{SESSION_PREFIX}-{}-{}",
        u32::MAX,
        uuid::Uuid::new_v4()
    ));
    super::super::create_private_directory(&legacy_stale).unwrap();
    fs::write(legacy_stale.join("auth.json"), b"legacy-synthetic-auth").unwrap();

    let legacy_live = root.path().join(format!(
        "{SESSION_PREFIX}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    super::super::create_private_directory(&legacy_live).unwrap();
    fs::write(legacy_live.join("auth.json"), b"live-legacy-auth").unwrap();

    let child_root = root.path().to_string_lossy().into_owned();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("active_session_child_holds_recovery_lock")
        .arg("--nocapture")
        .env(CHILD_ROOT_ENV, &child_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_file(&root.path().join(CHILD_ACTIVE_PATH), &mut child);
    let active_path =
        PathBuf::from(fs::read_to_string(root.path().join(CHILD_ACTIVE_PATH)).unwrap());
    assert!(active_path.exists());

    let recovered = PrivateDirectory::create_session_at(root.path()).unwrap();
    assert!(active_path.exists(), "live child session was reclaimed");
    assert!(
        !stale.exists(),
        "an unlocked session with a copied auth file should be reclaimed"
    );
    assert!(
        !legacy_stale.exists(),
        "legacy session from a dead process should be reclaimed"
    );
    assert!(
        legacy_live.join("auth.json").exists(),
        "a legacy session owned by a live process must be preserved"
    );
    assert!(recovered.path.exists());

    fs::write(root.path().join(CHILD_RELEASE), b"release").unwrap();
    assert!(child.wait().unwrap().success());
    assert!(
        !active_path.exists(),
        "child session should clean up on exit"
    );
}

#[test]
fn active_session_child_holds_recovery_lock() {
    let Ok(root) = std::env::var(CHILD_ROOT_ENV) else {
        return;
    };
    let root = Path::new(&root);
    let session = PrivateDirectory::create_session_at(root).unwrap();
    fs::write(
        root.join(CHILD_ACTIVE_PATH),
        session.path.to_string_lossy().as_bytes(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join(CHILD_RELEASE).exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_file(path: &Path, child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("session child exited before creating its active directory: {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        path.exists(),
        "session child did not publish its active path"
    );
}
