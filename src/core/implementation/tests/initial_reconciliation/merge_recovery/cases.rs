use super::*;

#[test]
fn preexisting_dirty_work_is_snapshotted_before_the_pinned_merge_retry() {
    let s = Sandbox::new();
    let (remote, local, common) = make_divergent(&s);
    super::super::save_clone_reconciliation_state(
        &s, &s.ticket, &s.repo, "main", &local, &remote, &common,
    );
    let worktree = super::current_task_repository(&s);
    fs::write(worktree.join("operator-note.txt"), "keep this draft\n").unwrap();
    let (calls, agent) = agent();

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(result.status, ImplementationStatus::AwaitingApproval);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (_, snapshot) = recovery_snapshot(&s);
    assert_eq!(snapshot["phase"], "recovered_once");
    assert!(
        snapshot["untracked_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "operator-note.txt")
    );
    let stash = snapshot["stash_commit"].as_str().unwrap();
    assert_eq!(
        s.git(
            &worktree,
            &["show", &format!("{stash}^3:operator-note.txt")]
        ),
        "keep this draft"
    );
}

#[test]
fn histories_already_in_task_base_skip_duplicate_reconciliation() {
    let s = Sandbox::new();
    fs::write(s.repo.join("prerequisite.txt"), "already included\n").unwrap();
    s.git(&s.repo, &["add", "prerequisite.txt"]);
    s.git(&s.repo, &["commit", "-qm", "complete prerequisite"]);
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    let remote = s.advance_remote();
    s.git(&s.repo, &["fetch", "-q", "origin", "main"]);
    let common = s.git(&s.repo, &["merge-base", &local, &remote]);
    super::super::save_clone_reconciliation_state(
        &s, &s.ticket, &s.repo, "main", &local, &remote, &common,
    );
    let dir = state_dir(&s.planning_store.root, &s.ticket).unwrap();
    let (calls, agent) = agent();

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "only the implementation agent should run"
    );
    assert_eq!(result.base_commit, remote);
    assert!(result.task_repository.join("prerequisite.txt").exists());
    let plan = crate::core::implementation::initial_reconciliation::load_plan(&dir)
        .unwrap()
        .unwrap();
    assert_eq!(plan.verified_commit.as_deref(), Some(remote.as_str()));
}
