use super::*;

#[path = "resume_and_pr/branch_intent.rs"]
mod branch_intent;

#[test]
fn cancelled_worktree_is_reviewed_and_resumed() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Interrupted);
    assert!(!s.root.join("pr-created").exists());
    s.advance_remote();
    let resumed = s.run("resume", calls.clone()).unwrap();
    assert_eq!(resumed.worktree, state.worktree);
    assert_eq!(resumed.base_commit, state.base_commit);
    assert!(!resumed.worktree.join("upstream.txt").exists());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn accepting_resume_moves_blocked_state_to_preparing_and_keeps_failure_history() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls).is_err());
    let mut blocked = load(&s.repo, &s.ticket).unwrap();
    blocked.status = ImplementationStatus::Blocked;
    blocked.detail = "NuGet audit feed was unreachable".into();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    save(&dir, &blocked).unwrap();

    let resumed = crate::core::implementation::mark_resume_started(&s.repo, &s.ticket)
        .unwrap()
        .unwrap();

    assert_eq!(resumed.status, ImplementationStatus::Preparing);
    assert!(resumed.detail.contains("Resume accepted"));
    assert!(resumed.detail.contains("NuGet audit feed was unreachable"));
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Preparing
    );
    let history = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-resume-context.txt")
        })
        .unwrap();
    assert_eq!(
        fs::read_to_string(history).unwrap(),
        "NuGet audit feed was unreachable"
    );

    let failed = crate::core::implementation::record_failed_attempt(
        &s.repo,
        &s.ticket,
        "Retry failed before implementation began: task lock is busy.",
    )
    .unwrap()
    .unwrap();
    assert_eq!(failed.status, ImplementationStatus::Blocked);
    assert_eq!(
        failed.detail,
        "Retry failed before implementation began: task lock is busy."
    );
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Blocked
    );
}

#[test]
fn active_resume_overrides_a_persisted_blocked_board_status() {
    let s = Sandbox::new();
    let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    state.status = ImplementationStatus::Blocked;

    assert_eq!(board_column(Some(&state), true), 1);
    assert_eq!(board_column(Some(&state), false), 3);
}

#[test]
fn feature_named_workspace_ticket_resumes_preserved_work() {
    let mut s = Sandbox::new();
    let ticket = ".koolade-packet/planning/tasks/feature/CHG-003-TASK-verify-workspace.md";
    fs::create_dir_all(s.repo.join(ticket).parent().unwrap()).unwrap();
    fs::rename(s.repo.join(&s.ticket), s.repo.join(ticket)).unwrap();
    s.ticket = ticket.into();
    s.git(&s.repo, &["add", "."]);
    s.git(&s.repo, &["commit", "-qm", "feature-named ticket"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let mut state = load(&s.repo, ticket).unwrap();
    state.status = ImplementationStatus::Blocked;
    save(&state_dir(&s.repo, ticket).unwrap(), &state).unwrap();
    let resumed = s.run("resume", calls.clone()).unwrap();
    assert_eq!(resumed.worktree, state.worktree);
    assert_eq!(resumed.base_commit, state.base_commit);
    assert_eq!(resumed.status, ImplementationStatus::AwaitingReview);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn implementation_reads_only_canonical_board_task_paths() {
    let s = Sandbox::new();
    let root = ".koolade-packet/planning/tasks";
    fs::create_dir_all(s.repo.join(root).join("validation")).unwrap();
    for name in [
        "001-task.md",
        "CHG-003-TASK-verify.md",
        "F10-TASK-verify.md",
        "README.md",
        "specification.md",
        "001-task.txt",
        "invalid-TASK-verify.md",
    ] {
        let path = format!("{root}/validation/{name}");
        fs::write(s.repo.join(&path), "# Fixture task").unwrap();
        assert_eq!(
            read_ticket(&s.repo, &path).is_ok(),
            crate::artifacts::task_docs::is_task_story_filename(name),
            "{path}"
        );
    }
    fs::create_dir_all(s.repo.join("planning/tasks/validation")).unwrap();
    fs::write(
        s.repo.join("planning/tasks/validation/001-task.md"),
        "# Legacy fixture task",
    )
    .unwrap();
    assert!(read_ticket(&s.repo, "planning/tasks/validation/001-task.md").is_err());
    assert!(read_ticket(&s.repo, ".koolade-packet/planning/tasks/../001-task.md").is_err());
}

#[test]
fn new_task_includes_latest_remote_without_touching_checkout() {
    let s = Sandbox::new();
    let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let latest = s.advance_remote();
    fs::write(s.repo.join("draft.txt"), "keep my draft").unwrap();
    let result = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    assert_eq!(result.base_commit, latest);
    assert_eq!(
        fs::read_to_string(result.worktree.join("upstream.txt")).unwrap(),
        "latest upstream\n"
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
    assert_eq!(
        fs::read_to_string(s.repo.join("draft.txt")).unwrap(),
        "keep my draft"
    );
    assert!(!s.repo.join("upstream.txt").exists());
}

#[test]
fn fetch_failure_stops_before_agent_runs() {
    let s = Sandbox::new();
    s.git(
        &s.repo,
        &["remote", "set-url", "origin", "/nonexistent-koolade-remote"],
    );
    let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let error = s.run("complete", calls.clone()).unwrap_err().to_string();
    assert!(error.contains("failed"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
    assert!(load(&s.repo, &s.ticket).is_none());
}

#[test]
fn local_commits_ahead_of_remote_are_preserved() {
    let s = Sandbox::new();
    fs::write(s.repo.join("local.txt"), "local change").unwrap();
    s.git(&s.repo, &["add", "."]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    assert_eq!(state.base_commit, original);
    assert!(state.worktree.join("local.txt").exists());
}

#[test]
fn pr_checks_persist_closed_reopened_merged_and_keep_state_on_failure() {
    let s = Sandbox::new();
    s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let mut held = load(&s.repo, &s.ticket).unwrap();
    held.detail =
        "Verified locally. No remote changes were made; share for review when ready.".into();
    save(&dir, &held).unwrap();
    let (progress, _rx) = mpsc::channel();
    let runner = Runner {
        gh: s.gh.to_string_lossy().into(),
        deadline: Instant::now() + Duration::from_secs(20),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    let merge_commit = "0123456789abcdef0123456789abcdef01234567";
    for (value, column) in [("CLOSED", 3), ("OPEN", 2), ("MERGED", 4)] {
        let response = if value == "MERGED" {
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{{\"state\":\"MERGED\",\"mergeCommit\":{{\"oid\":\"{merge_commit}\"}}}}'\n"
            )
        } else {
            format!("#!/bin/sh\nprintf '%s\\n' '{{\"state\":\"{value}\"}}'\n")
        };
        fs::write(&s.gh, response).unwrap();
        refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
        let state = load(&s.repo, &s.ticket).unwrap();
        assert_eq!(state.pr_state.map(PullRequestState::label), Some(value));
        assert_eq!(board_column(Some(&state), false), column);
        assert!(state.pr_checked_at.is_some());
        assert!(state.pr_check_error.is_none());
        assert!(!state.detail.contains("No remote changes were made"));
        assert!(state.detail.contains("Pull request status:"));
        if value == "MERGED" {
            assert_eq!(state.merged_commit.as_deref(), Some(merge_commit));
            assert!(
                state
                    .detail
                    .contains("Pull request status: Merged as 0123456789ab")
            );
        }
        if value != "MERGED" {
            for output in ["echo offline >&2; exit 1", "echo '{\"state\":\"UNKNOWN\"}'"] {
                fs::write(&s.gh, format!("#!/bin/sh\n{output}\n")).unwrap();
                refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
                let failed = load(&s.repo, &s.ticket).unwrap();
                assert_eq!(failed.pr_state, state.pr_state);
                assert_eq!(failed.pr_checked_at, state.pr_checked_at);
                assert_eq!(failed.detail, state.detail);
                assert!(failed.pr_check_error.is_some());
            }
        }
    }
    assert_eq!(load_all(&s.repo).len(), 1);
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Completed
    );
    let mut stale = load(&s.repo, &s.ticket).unwrap();
    stale.detail =
        "Verified locally. No remote changes were made; share for review when ready.".into();
    save(&dir, &stale).unwrap();
    fs::write(
        &s.gh,
        "#!/bin/sh\necho unexpected GitHub request >&2; exit 1\n",
    )
    .unwrap();
    refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
    let migrated = load(&s.repo, &s.ticket).unwrap();
    assert!(!migrated.detail.contains("No remote changes were made"));
    assert!(
        migrated
            .detail
            .contains("Pull request status: Merged as 0123456789ab")
    );
}

#[test]
fn approval_and_change_request_states_stay_in_review() {
    let s = Sandbox::new();
    let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    state.pr_url = None;
    state.pr_state = None;
    for status in [
        ImplementationStatus::AwaitingApproval,
        ImplementationStatus::ChangesRequested,
    ] {
        state.status = status;
        assert_eq!(board_column(Some(&state), false), 2);
    }
}

#[test]
fn pr_refresh_does_not_overwrite_an_active_implementation() {
    let s = Sandbox::new();
    s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let before = fs::read(dir.join("state.json")).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("run.lock"))
        .unwrap();
    lock.lock().unwrap();
    let (progress, _rx) = mpsc::channel();
    let runner = Runner {
        gh: "must-not-run".into(),
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    let started = Instant::now();
    refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
    // The deliberate no-op must stay bounded: a held lock may add at most
    // the short settle budget, never an unbounded block.
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(fs::read(dir.join("state.json")).unwrap(), before);
}
#[test]
fn blocked_or_failed_verification_never_creates_pr() {
    for mode in ["blocked", "fail"] {
        let s = Sandbox::new();
        let outcome = s.run(mode, Arc::new(AtomicUsize::new(0)));
        assert!(
            outcome.is_err(),
            "mode {mode} must fail: {:?}",
            outcome.ok()
        );
        assert!(!s.root.join("pr-created").exists());
        let Some(state) = load(&s.repo, &s.ticket) else {
            panic!(
                "mode {mode}: run failed ({outcome:?}) before persisting workflow state; \n\
                 a git/io-level infrastructure fault is suspected rather than the \n\
                 verification mode under test"
            );
        };
        assert!(state.worktree.join("implemented.txt").exists());
    }
}
#[test]
fn external_decision_stops_after_one_report_and_preserves_work() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let result = s.run("external_blocked", calls.clone());
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert!(
        state
            .detail
            .contains("Adjudicator: approve the revised contract")
    );
    assert!(state.worktree.join("implemented.txt").exists());
    assert!(!s.root.join("pr-created").exists());
}
#[test]
fn feature_id_collision_does_not_inject_another_features_specification() {
    assert!(!specification_matches_task(
        "# Task\n\nFeature: Switch Workspaces\n",
        "# CHG-003: Readable Chat Replies\n"
    ));
    assert!(specification_matches_task(
        "# Task\n\nFeature: Switch Workspaces\n",
        "# CHG-003: Switch Workspaces\n"
    ));
}
