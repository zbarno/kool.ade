use super::*;

#[test]
fn preexisting_dirty_work_is_preserved_and_never_auto_stashed() {
    let s = Sandbox::new();
    let (remote, _, _) = make_divergent(&s);
    let worktree = task_worktree(&s);
    fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    let key = crate::core::implementation::key_for_ticket(&s.ticket);
    let branch = format!("koolade/{key}");
    s.git(
        &s.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_str().unwrap(),
            &remote,
        ],
    );
    fs::write(worktree.join("operator-note.txt"), "keep this draft\n").unwrap();
    let (calls, agent) = agent();

    let error = run_with_agent(&s, &agent, None).unwrap_err().to_string();

    assert!(
        error.contains("outside the two pinned histories"),
        "{error}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fs::read_to_string(worktree.join("operator-note.txt")).unwrap(),
        "keep this draft\n"
    );
    assert!(
        !state_dir(&s.repo, &s.ticket)
            .unwrap()
            .join("base-reconciliation-recovery.json")
            .exists()
    );
    assert!(
        !std::process::Command::new("git")
            .args(["rev-parse", "--verify", "MERGE_HEAD"])
            .current_dir(&worktree)
            .output()
            .unwrap()
            .status
            .success()
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
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    crate::core::implementation::initial_reconciliation::save_plan(
        &dir,
        "main",
        &local,
        &remote,
        &common,
        &[],
    )
    .unwrap();
    let (calls, agent) = agent();

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "only the implementation agent should run"
    );
    assert_eq!(result.base_commit, remote);
    assert!(result.worktree.join("prerequisite.txt").exists());
    let plan = crate::core::implementation::initial_reconciliation::load_plan(&dir)
        .unwrap()
        .unwrap();
    assert_eq!(plan.verified_commit.as_deref(), Some(remote.as_str()));
}
