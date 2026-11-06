use super::*;
use crate::core::implementation::{ImplementationStatus, TaskRepositoryKind};
use std::fs;

fn temp_repo() -> std::path::PathBuf {
    let repo = std::env::temp_dir().join(format!(
        "koolade-activity-recovery-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&repo).unwrap();
    repo
}

fn unfinished_activity() -> crate::harness::LiveProgress {
    crate::harness::LiveProgress {
        telemetry: crate::harness::ActivityTelemetry {
            started_ms: Some(10),
            updated_ms: Some(20),
            updates: 1,
            ..Default::default()
        },
        activity: Some("Implementation cancelled".into()),
        ..Default::default()
    }
}

#[test]
fn terminal_interruption_finishes_stale_activity() {
    let repo = temp_repo();
    let mut activity = unfinished_activity();
    assert!(finalize_terminal_activity_if_stale(
        &repo,
        "planning/tasks/001-task.md",
        None,
        ImplementationStatus::Interrupted,
        &mut activity,
        30,
    ));
    assert_eq!(activity.telemetry.finished_ms, Some(30));
    assert_eq!(activity.telemetry.updated_ms, Some(30));
    assert_eq!(activity.activity.as_deref(), Some("Interrupted"));
    fs::remove_dir_all(repo).unwrap();
}

#[test]
fn nonterminal_status_does_not_finish_activity() {
    let repo = temp_repo();
    let mut activity = unfinished_activity();
    assert!(!finalize_terminal_activity_if_stale(
        &repo,
        "planning/tasks/001-task.md",
        None,
        ImplementationStatus::Implementing,
        &mut activity,
        30,
    ));
    assert_eq!(activity.telemetry.finished_ms, None);
    fs::remove_dir_all(repo).unwrap();
}

#[test]
fn checklist_progress_survives_activity_reload_and_terminal_states() {
    let repo = temp_repo();
    let ticket = "planning/tasks/001-task.md";
    let mut progress = unfinished_activity();
    progress.checklist = vec![0, 2];
    progress.checklist_revision = 42;
    save_activity(&repo, ticket, &progress).unwrap();

    let loaded = load_activity(&repo, ticket).unwrap();
    assert_eq!(loaded.checklist, [0, 2]);
    assert_eq!(loaded.checklist_revision, 42);
    let mut resumed = loaded;
    resumed.update(crate::harness::LiveProgress {
        checklist: vec![0, 2, 3],
        checklist_revision: 43,
        ..Default::default()
    });
    resumed.update(crate::harness::LiveProgress {
        checklist: vec![0],
        checklist_revision: 42,
        ..Default::default()
    });
    assert_eq!(resumed.checklist, [0, 2, 3]);
    save_activity(&repo, ticket, &resumed).unwrap();
    for status in [
        ImplementationStatus::Blocked,
        ImplementationStatus::Interrupted,
        ImplementationStatus::AwaitingReview,
        ImplementationStatus::Completed,
    ] {
        let mut resumed = load_activity(&repo, ticket).unwrap();
        finalize_terminal_activity_if_stale(&repo, ticket, None, status, &mut resumed, 30);
        assert_eq!(resumed.checklist, [0, 2, 3]);
        save_activity(&repo, ticket, &resumed).unwrap();
        assert_eq!(load_activity(&repo, ticket).unwrap().checklist, [0, 2, 3]);
    }
    fs::remove_dir_all(repo).unwrap();
}

#[test]
fn historical_activity_is_sanitized_before_rendering_and_rewritten_locally() {
    let repo = temp_repo();
    let ticket = "planning/tasks/001-task.md";
    let dir = super::super::state_dir(&repo, ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let legacy = crate::harness::LiveProgress {
        thoughts: "private reasoning marker".into(),
        response: "API_KEY=synthetic-old-secret /home/alice/private.rs".into(),
        posts: vec![crate::harness::LivePost {
            id: (1, 0),
            kind: "tool".into(),
            text: "\u{1b}[31mTOKEN=synthetic-old-token\u{1b}[0m /home/alice/secret.rs".into(),
            recorded_at_ms: Some(10),
            run_id: None,
        }],
        ..Default::default()
    };
    fs::write(
        dir.join("activity.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();

    let loaded = load_activity(&repo, ticket).unwrap();
    assert!(loaded.thoughts.is_empty());
    assert!(!loaded.response.contains("synthetic-old-secret"));
    assert!(!loaded.response.contains("/home/alice"));
    assert!(!loaded.posts[0].text.contains("synthetic-old-token"));
    assert!(!loaded.posts[0].text.contains('\u{1b}'));
    let rewritten = fs::read_to_string(dir.join("activity.json")).unwrap();
    assert!(!rewritten.contains("synthetic-old-secret"));
    assert!(!rewritten.contains("synthetic-old-token"));
    assert!(!rewritten.contains("private reasoning marker"));
    fs::remove_dir_all(repo).unwrap();
}

#[test]
fn live_worker_lock_prevents_premature_activity_finish() {
    let repo = temp_repo();
    let ticket = "planning/tasks/001-task.md";
    let dir = super::super::state_dir(&repo, ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("run.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let mut activity = unfinished_activity();
    assert!(!finalize_terminal_activity_if_stale(
        &repo,
        ticket,
        None,
        ImplementationStatus::Interrupted,
        &mut activity,
        30,
    ));
    assert_eq!(activity.telemetry.finished_ms, None);
    drop(lock);
    assert!(finalize_terminal_activity_if_stale(
        &repo,
        ticket,
        None,
        ImplementationStatus::Interrupted,
        &mut activity,
        40,
    ));
    fs::remove_dir_all(repo).unwrap();
}

#[path = "tests/identity.rs"]
mod identity;
