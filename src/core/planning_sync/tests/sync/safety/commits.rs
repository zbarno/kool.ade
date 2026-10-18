use super::super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::{
    fs,
    io::{BufRead, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn manual_sync_does_not_report_synced_with_an_unjournaled_local_commit() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("external-local-commit");
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "external committed edit",
    );
    super::super::super::support::commit_all(&fixture.first, "external planning commit");

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Pending);
    assert!(
        status
            .detail
            .as_deref()
            .unwrap()
            .contains("local planning edits have not been journaled")
    );
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "original"
    );
}

#[test]
fn local_commit_after_a_successful_publish_stays_pending_on_sibling_history() {
    let fixture = Fixture::new();
    let state_root = fixture.state_root("post-publish-local-commit");
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );

    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "published app edit",
    );
    let operation = prepare_local_intent(
        &fixture.first,
        &state_root,
        "published app edit",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    super::super::super::support::commit_all(&fixture.first, "local app checkpoint");
    crate::core::planning_sync::runner::finish_shared_intent_at(
        &fixture.first,
        &state_root,
        operation,
    );
    let published = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(published.state, PlanningSyncState::Synced);
    let local_commit = crate::core::planning_sync::git::head(&fixture.first).unwrap();
    assert_ne!(
        published.confirmed_commit.as_deref(),
        Some(local_commit.as_str())
    );

    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "detail",
        "external committed edit",
    );
    super::super::super::support::commit_all(&fixture.first, "external planning checkpoint");
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Pending);
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "published app edit"
    );
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["detail"],
        "stable"
    );
    assert!(
        status
            .detail
            .as_deref()
            .unwrap()
            .contains("local planning edits have not been journaled")
    );
}

#[test]
fn manual_sync_waits_for_an_in_progress_repository_commit() {
    const REPO: &str = "KOOLADE_SYNC_LOCK_TEST_REPO";
    const SIGNAL: &str = "KOOLADE_SYNC_LOCK_TEST_SIGNAL";
    if let Some(repo) = std::env::var_os(REPO) {
        let _lock = crate::core::gitops::lock_repository(std::path::Path::new(&repo)).unwrap();
        fs::write(std::env::var_os(SIGNAL).unwrap(), "locked").unwrap();
        let mut release = String::new();
        std::io::stdin().lock().read_line(&mut release).unwrap();
        return;
    }

    let fixture = Fixture::new();
    let state_root = fixture.state_root("sync-repository-lock");
    assert_eq!(
        sync_with_state_root(&fixture.first, &state_root)
            .unwrap()
            .state,
        PlanningSyncState::Synced
    );

    fixture.write_work(&fixture.second, fixture.work_id, "title", "remote title");
    super::super::super::support::commit_all(&fixture.second, "remote title");
    super::super::super::support::git(
        Some(&fixture.second),
        &["push", "origin", "HEAD:refs/heads/main"],
    );

    let signal = fixture.root.join("repository-lock-held");
    let helper_test = format!(
        "{}::manual_sync_waits_for_an_in_progress_repository_commit",
        std::thread::current()
            .name()
            .unwrap()
            .rsplit_once("::")
            .unwrap()
            .0
    );
    let mut lock_holder = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &helper_test, "--nocapture"])
        .env(REPO, &fixture.first)
        .env(SIGNAL, &signal)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !signal.exists() {
        if let Some(status) = lock_holder.try_wait().unwrap() {
            panic!("repository lock holder exited before acquiring the lock: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "repository lock holder did not start"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let repo = fixture.first.clone();
    let worker_state_root = state_root.clone();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = sync_with_state_root(&repo, &worker_state_root);
        let _ = done_tx.send(result);
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let early = done_rx.recv_timeout(Duration::from_millis(200)).ok();
    let unchanged: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.first.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    let unchanged_title = unchanged["data"]["title"] == "original";
    lock_holder
        .stdin
        .take()
        .unwrap()
        .write_all(b"release\n")
        .unwrap();
    assert!(lock_holder.wait().unwrap().success());
    assert!(
        early.is_none(),
        "manual sync returned while another process held the repository lock"
    );
    assert!(
        unchanged_title,
        "manual sync changed records before lock release"
    );
    let status = done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(status.state, PlanningSyncState::Synced);
    let refreshed: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.first.join(work_path(fixture.work_id))).unwrap())
            .unwrap();
    assert_eq!(refreshed["data"]["title"], "remote title");
}
