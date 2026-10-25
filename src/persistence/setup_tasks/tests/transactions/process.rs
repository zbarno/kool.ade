use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::app::setup_attention::catalog::FindingStatus;

use super::super::super::{
    AttemptState, FailureCode, SetupTaskStateV1, SetupTaskTransitionV1 as Transition,
};
use super::*;

/// Production boundary: startup recovery and lifecycle writes in two app
/// processes share one machine-local store. A unit-only lock cannot prove that
/// another process preserves a live owner's approval and verification states,
/// or interrupts them after exit. Setup: two child test processes, temporary
/// state files, no network, Docker, or SSH dependency; expected cost is low.
#[test]
fn startup_preserves_live_owner_then_interrupts_persisted_attempts_after_exit() {
    const PATH_ENV: &str = "KOOLADE_TEST_SETUP_OWNER_PATH";
    const READY_ENV: &str = "KOOLADE_TEST_SETUP_OWNER_READY";
    const RELEASE_ENV: &str = "KOOLADE_TEST_SETUP_OWNER_RELEASE";

    if let (Some(path), Some(ready), Some(release)) = (
        std::env::var_os(PATH_ENV),
        std::env::var_os(READY_ENV),
        std::env::var_os(RELEASE_ENV),
    ) {
        let path = PathBuf::from(path);
        transaction::apply_transition_at(
            &path,
            Transition::RequestAttempt {
                task_id: TASK_ID.into(),
                operation_id: OPERATION_ID.into(),
                version: 1,
                requested_scopes: vec![TASK_ID.into()],
                at_unix_ms: 11,
            },
        )
        .unwrap();
        transaction::apply_transition_at(
            &path,
            Transition::RequestAttempt {
                task_id: OTHER_TASK_ID.into(),
                operation_id: OTHER_OPERATION_ID.into(),
                version: 1,
                requested_scopes: vec![OTHER_TASK_ID.into()],
                at_unix_ms: 12,
            },
        )
        .unwrap();
        transaction::apply_transition_at(
            &path,
            Transition::RecordAuthorization {
                task_id: OTHER_TASK_ID.into(),
                operation_id: OTHER_OPERATION_ID.into(),
                binding: binding(OTHER_TASK_ID, OTHER_OPERATION_ID),
                at_unix_ms: 13,
            },
        )
        .unwrap();
        transaction::apply_transition_at(
            &path,
            Transition::AwaitVerification {
                task_id: OTHER_TASK_ID.into(),
                operation_id: OTHER_OPERATION_ID.into(),
                at_unix_ms: 14,
            },
        )
        .unwrap();
        fs::write(ready, b"ready").unwrap();
        let release = PathBuf::from(release);
        let deadline = Instant::now() + Duration::from_secs(15);
        while !release.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(release.exists(), "parent did not release the owner child");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let path = transaction::path_for_state_root(temp.path());
    seed_ready(&path, &[TASK_ID, OTHER_TASK_ID]);
    let ready = temp.path().join("owner-ready");
    let release = temp.path().join("owner-release");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("persistence::setup_tasks::tests::transactions::process::startup_preserves_live_owner_then_interrupts_persisted_attempts_after_exit")
        .arg("--nocapture")
        .env(PATH_ENV, &path)
        .env(READY_ENV, &ready)
        .env(RELEASE_ENV, &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_file(&ready, &mut child);

    let live = transaction::recover_interrupted_at(&path, 20).unwrap();
    assert_eq!(live.tasks[TASK_ID].state, SetupTaskStateV1::NeedsApproval);
    assert_eq!(
        live.tasks[TASK_ID].attempts[0].state,
        AttemptState::NeedsApproval
    );
    assert_eq!(
        live.tasks[OTHER_TASK_ID].state,
        SetupTaskStateV1::AwaitingVerification
    );
    assert_eq!(
        live.tasks[OTHER_TASK_ID].attempts[0].state,
        AttemptState::AwaitingVerification
    );
    assert!(
        live.tasks[OTHER_TASK_ID].attempts[0]
            .authorization_binding
            .is_some()
    );

    fs::write(&release, b"exit").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "owner child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let generation = transaction::begin_scan_at(&path).unwrap();
    let checked_at = crate::persistence::harness_settings::current_unix_ms();
    let recovered = transaction::reconcile_live_at(
        &path,
        &snapshot(
            &[
                (TASK_ID, FindingStatus::NeedsAttention, &[]),
                (OTHER_TASK_ID, FindingStatus::NeedsAttention, &[]),
            ],
            &[],
            checked_at,
        ),
        generation,
    )
    .unwrap();
    for task_id in [TASK_ID, OTHER_TASK_ID] {
        let task = &recovered.tasks[task_id];
        assert_eq!(task.state, SetupTaskStateV1::ReadyToFix);
        assert_eq!(task.attempts[0].state, AttemptState::Interrupted);
        assert_eq!(
            task.attempts[0].failure,
            Some(FailureCode::InterruptedByRestart)
        );
        assert!(task.attempts[0].authorization_binding.is_none());
        assert!(task.attempts[0].owner_instance_id.is_none());
    }
    assert_eq!(transaction::load_from(&path).unwrap().0, recovered);
    assert_eq!(
        transaction::recover_interrupted_at(&path, checked_at + 1).unwrap(),
        recovered
    );
    assert!(
        transaction::apply_transition_at(
            &path,
            Transition::RecordAuthorization {
                task_id: TASK_ID.into(),
                operation_id: OPERATION_ID.into(),
                binding: binding(TASK_ID, OPERATION_ID),
                at_unix_ms: checked_at + 2,
            },
        )
        .is_err()
    );
    assert!(
        transaction::apply_transition_at(
            &path,
            Transition::AwaitVerification {
                task_id: OTHER_TASK_ID.into(),
                operation_id: OTHER_OPERATION_ID.into(),
                at_unix_ms: 23,
            },
        )
        .is_err()
    );
}

/// Production boundary: concurrent app processes reserve scan generations
/// through the public transaction entry point. A thread-only test cannot prove
/// the OS file lock prevents lost updates. Setup: six child test processes,
/// thirty small durable JSON writes, a temporary KOOLADE_HOME, and no external
/// services; expected cost is moderate due to process startup and fsync.
#[test]
fn concurrent_process_scan_writers_do_not_lose_generation_updates() {
    const GO_ENV: &str = "KOOLADE_TEST_SETUP_SCAN_GO";
    const READY_ENV: &str = "KOOLADE_TEST_SETUP_SCAN_READY";
    if let (Some(go), Some(ready)) = (std::env::var_os(GO_ENV), std::env::var_os(READY_ENV)) {
        let go = PathBuf::from(go);
        fs::write(ready, b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !go.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(go.exists(), "parent did not release scan writers");
        for _ in 0..5 {
            crate::persistence::setup_tasks::begin_scan().unwrap();
        }
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let state_root = temp.path().join("state");
    fs::create_dir_all(&state_root).unwrap();
    let go = temp.path().join("go");
    let ready_paths = (0..6)
        .map(|worker| temp.path().join(format!("ready-{worker}")))
        .collect::<Vec<_>>();
    let mut children = (0..6)
        .map(|worker| {
            Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("persistence::setup_tasks::tests::transactions::process::concurrent_process_scan_writers_do_not_lose_generation_updates")
                .arg("--nocapture")
                .env("KOOLADE_HOME", &state_root)
                .env(GO_ENV, &go)
                .env(READY_ENV, &ready_paths[worker])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for (child, ready) in children.iter_mut().zip(&ready_paths) {
        wait_for_file(ready, child);
    }
    fs::write(&go, b"start").unwrap();
    for (index, child) in children.into_iter().enumerate() {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "writer {index} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let path = transaction::path_for_state_root(&state_root);
    let store = transaction::load_from(&path).unwrap().0;
    assert_eq!(store.latest_scan_generation, 30);
}

fn wait_for_file(path: &Path, child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("owner child exited early with {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "owner child did not reach active state");
}
