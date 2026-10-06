use super::*;

/// Task-only activity survives reconnects without becoming a tracked artifact.
pub fn load_activity(repo: &Path, ticket: &str) -> Option<crate::harness::LiveProgress> {
    serde_json::from_slice(&fs::read(state_dir(repo, ticket).ok()?.join("activity.json")).ok()?)
        .ok()
}
pub fn save_activity(
    repo: &Path,
    ticket: &str,
    activity: &crate::harness::LiveProgress,
) -> anyhow::Result<()> {
    let dir = state_dir(repo, ticket)?;
    fs::create_dir_all(&dir)?;
    // Persist a bounded view: in-memory snapshots keep the full history, but
    // the on-disk snapshot keeps only trailing windows so a long turn cannot
    // bloat activity.json (quadratic rewrites of ever-larger files were a
    // major source of disk wear).
    crate::artifacts::atomic_write_bytes(
        &dir.join("activity.json"),
        &serde_json::to_vec(&trim_for_persist(activity))?,
    )
}

/// Close a persisted activity snapshot after its implementation record has
/// reached a terminal state and its worker no longer holds the task lock.
/// This repairs snapshots left unfinished when a window disconnects before it
/// can consume the worker's final event.
pub fn finalize_terminal_activity_if_stale(
    repo: &Path,
    ticket: &str,
    task_uid: Option<&str>,
    status: super::ImplementationStatus,
    progress: &mut crate::harness::LiveProgress,
    finished_ms: i64,
) -> bool {
    if !matches!(
        status,
        super::ImplementationStatus::Blocked
            | super::ImplementationStatus::Interrupted
            | super::ImplementationStatus::ReadyToPublish
            | super::ImplementationStatus::AwaitingApproval
            | super::ImplementationStatus::ChangesRequested
            | super::ImplementationStatus::AwaitingReview
            | super::ImplementationStatus::Completed
            | super::ImplementationStatus::PullRequestClosed
    ) || progress.telemetry.finished_ms.is_some()
        || !has_activity(progress)
        || worker_holds_lock(repo, ticket, task_uid)
    {
        return false;
    }
    progress.telemetry.updated_ms = Some(finished_ms);
    progress.telemetry.finished_ms = Some(finished_ms);
    progress.activity = Some(status.label().to_owned());
    true
}

fn has_activity(progress: &crate::harness::LiveProgress) -> bool {
    progress.telemetry.started_ms.is_some()
        || progress.telemetry.updated_ms.is_some()
        || progress.telemetry.updates > 0
        || progress.activity.is_some()
        || !progress.posts.is_empty()
        || !progress.thoughts.is_empty()
        || !progress.response.is_empty()
}

fn worker_holds_lock(repo: &Path, ticket: &str, task_uid: Option<&str>) -> bool {
    let Ok(dir) = super::state_dir_for_task(repo, ticket, task_uid) else {
        return true;
    };
    let file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("run.lock"))
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    if file.try_lock().is_err() {
        return true;
    }
    let _ = file.unlock();
    false
}

/// Bounds for the persisted activity snapshot (see [`save_activity`]).
pub(super) const ACTIVITY_MAX_POSTS: usize = 200;
pub(super) const ACTIVITY_MAX_POST_CHARS: usize = 2_000;
pub(super) const ACTIVITY_MAX_FIELD_CHARS: usize = 32_000;

/// Build the bounded, persistence-shaped copy of a live snapshot.
/// Pure: the caller's in-memory state is never mutated.
pub(super) fn trim_for_persist(
    progress: &crate::harness::LiveProgress,
) -> crate::harness::LiveProgress {
    let mut out = progress.clone();
    if out.posts.len() > ACTIVITY_MAX_POSTS {
        let drop = out.posts.len() - ACTIVITY_MAX_POSTS;
        out.posts.drain(..drop);
    }
    for post in &mut out.posts {
        post.text = retain_suffix(&post.text, ACTIVITY_MAX_POST_CHARS);
    }
    out.thoughts = retain_suffix(&out.thoughts, ACTIVITY_MAX_FIELD_CHARS);
    out.response = retain_suffix(&out.response, ACTIVITY_MAX_FIELD_CHARS);
    out.specification = out
        .specification
        .as_deref()
        .map(|s| retain_suffix(s, ACTIVITY_MAX_FIELD_CHARS));
    out.activity = out
        .activity
        .as_deref()
        .map(|s| retain_suffix(s, ACTIVITY_MAX_FIELD_CHARS));
    out
}

/// Keep the LAST `max_chars` characters of `s` (the tail carries the newest
/// content), prefixing an elision marker when anything was dropped.
/// Character-boundary safe.
pub(super) fn retain_suffix(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_owned();
    }
    let drop = total - max_chars;
    let cut = s.char_indices().nth(drop).map(|(idx, _)| idx).unwrap_or(0);
    format!("\u{2026}{}", &s[cut..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::implementation::ImplementationStatus;

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

    #[test]
    fn stable_task_identity_finds_a_live_lock_after_path_change() {
        let repo = temp_repo();
        let old_ticket = "planning/tasks/old/001-task.md";
        let new_ticket = "planning/tasks/renamed/001-task.md";
        let task_uid = uuid::Uuid::new_v4().to_string();
        let dir = super::super::state_dir(&repo, old_ticket).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let state = crate::core::implementation::Implementation {
            ticket: old_ticket.into(),
            task_uid: Some(task_uid.clone()),
            ticket_text: String::new(),
            approved_specification: None,
            approved_product_context: None,
            completed_dependency_context: None,
            branch: "koolade/task".into(),
            base: "main".into(),
            base_commit: "base".into(),
            worktree: repo.join("worktree"),
            status: ImplementationStatus::Interrupted,
            detail: String::new(),
            pr_url: None,
            verified_head: None,
            auto_merge: false,
            merged_commit: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            independent_check: None,
            cleanup: Default::default(),
        };
        fs::write(dir.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
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
            new_ticket,
            Some(&task_uid),
            ImplementationStatus::Interrupted,
            &mut activity,
            30,
        ));
        assert_eq!(activity.telemetry.finished_ms, None);
        drop(lock);
        fs::remove_dir_all(repo).unwrap();
    }
}
