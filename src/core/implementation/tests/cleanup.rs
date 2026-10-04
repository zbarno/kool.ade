use super::*;

#[test]
fn cleanup_reclaims_ignored_builds_keeps_evidence_and_is_idempotent() {
    let s = Sandbox::new();
    let state = completed_cleanup_fixture(&s);
    fs::write(common(&s.repo).unwrap().join("info/exclude"), "target/\n").unwrap();
    fs::create_dir_all(state.worktree.join("target/debug")).unwrap();
    fs::write(
        state.worktree.join("target/debug/build-cache"),
        vec![0u8; 1024 * 1024],
    )
    .unwrap();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let evidence = fs::read(dir.join("verified-report.json")).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    let done = load(&s.repo, &s.ticket).unwrap();
    assert!(done.cleanup.completed_at.is_some(), "{:?}", done.cleanup);
    assert!(!state.worktree.exists());
    assert_eq!(
        fs::read(dir.join("verified-report.json")).unwrap(),
        evidence
    );
    assert_eq!(done.status, ImplementationStatus::Completed);
    assert_eq!(board_column(Some(&done), false), 4);
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert_eq!(load(&s.repo, &s.ticket).unwrap().cleanup, done.cleanup);
}

#[test]
fn cleanup_never_reclaims_an_unpublished_completion_commit() {
    let s = Sandbox::new();
    let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    state.status = ImplementationStatus::Completed;
    state.merged_commit = state.verified_head.clone();
    save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    let preserved = load(&s.repo, &s.ticket).unwrap();
    assert!(preserved.cleanup.error.unwrap().contains("not in origin"));
    assert!(state.worktree.join("implemented.txt").exists());
}

#[test]
fn cleanup_preserves_changes_and_retries_after_they_are_resolved() {
    let s = Sandbox::new();
    let state = completed_cleanup_fixture(&s);
    let draft = state.worktree.join("unsaved-draft.txt");
    fs::write(&draft, "keep this").unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    let failed = load(&s.repo, &s.ticket).unwrap();
    assert!(
        failed
            .cleanup
            .error
            .as_deref()
            .unwrap()
            .contains("local changes")
    );
    assert_eq!(fs::read_to_string(&draft).unwrap(), "keep this");
    assert_eq!(failed.status, ImplementationStatus::Completed);
    fs::rename(&draft, s.root.join("saved-draft.txt")).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .completed_at
            .is_some()
    );
    assert!(!state.worktree.exists());
}

#[test]
fn cleanup_preserves_changed_head_and_locked_worktree() {
    let s = Sandbox::new();
    let state = completed_cleanup_fixture(&s);
    s.git(
        &s.repo,
        &["worktree", "lock", state.worktree.to_str().unwrap()],
    );
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(load(&s.repo, &s.ticket).unwrap().cleanup.error.is_some());
    assert!(state.worktree.exists());
    s.git(
        &s.repo,
        &["worktree", "unlock", state.worktree.to_str().unwrap()],
    );
    s.git(
        &state.worktree,
        &["commit", "--allow-empty", "-qm", "new local work"],
    );
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .error
            .unwrap()
            .contains("changed HEAD")
    );
    assert!(state.worktree.exists());
}

#[test]
fn cleanup_preserves_wrong_identity_missing_publication_and_active_work() {
    let s = Sandbox::new();
    let mut state = completed_cleanup_fixture(&s);
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("run.lock"))
        .unwrap();
    lock.lock().unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(state.worktree.exists());
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .attempted_at
            .is_none()
    );
    drop(lock);
    state.worktree = s.repo.clone();
    save(&dir, &state).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .error
            .unwrap()
            .contains("allocation")
    );
    assert!(s.repo.join(&s.ticket).exists());
    state.merged_commit = None;
    save(&dir, &state).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .error
            .unwrap()
            .contains("No confirmed")
    );
}
