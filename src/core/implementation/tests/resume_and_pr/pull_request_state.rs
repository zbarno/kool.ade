use super::*;

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
        runtime_config_source: None,
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
        runtime_config_source: None,
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
