use super::*;

#[test]
fn an_interrupted_pinned_merge_is_snapshotted_and_retried() {
    let s = Sandbox::new();
    let (remote, local, common) = make_divergent(&s);
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
    s.git(
        &worktree,
        &["merge", "--no-ff", "--no-commit", "--no-edit", &local],
    );
    fs::write(worktree.join("upstream.txt"), "unstaged edit\n").unwrap();
    fs::write(worktree.join("recovery-staged.txt"), "staged extra\n").unwrap();
    s.git(&worktree, &["add", "recovery-staged.txt"]);
    fs::write(
        worktree.join("recovery-untracked.txt"),
        "interrupted edit\n",
    )
    .unwrap();
    fs::create_dir_all(worktree.join(".cache")).unwrap();
    fs::write(
        worktree.join(".cache/recovery-ignored.txt"),
        "ignored extra\n",
    )
    .unwrap();
    let (calls, agent) = agent();

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(result.status, ImplementationStatus::AwaitingReview);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (snapshot_path, snapshot) = recovery_snapshot(&s);
    assert_eq!(snapshot["phase"], "recovered_once");
    assert_eq!(snapshot["schema_version"], 2);
    assert_eq!(
        snapshot["task_repository"],
        worktree.to_string_lossy().as_ref()
    );
    assert!(snapshot.get("worktree").is_none());
    assert!(snapshot_path.exists());
    assert!(
        snapshot["staged_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "recovery-staged.txt")
    );
    assert!(
        snapshot["unstaged_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "upstream.txt")
    );
    assert!(
        snapshot["untracked_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "recovery-untracked.txt")
    );
    assert!(
        snapshot["ignored_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == ".cache/recovery-ignored.txt")
    );
    let stash = snapshot["stash_commit"].as_str().unwrap();
    assert_eq!(
        s.git(&s.repo, &["show", &format!("{stash}:upstream.txt")]),
        "unstaged edit"
    );
    assert_eq!(
        s.git(&s.repo, &["show", &format!("{stash}:recovery-staged.txt")]),
        "staged extra"
    );
    assert_eq!(
        s.git(
            &s.repo,
            &["show", &format!("{stash}^3:recovery-untracked.txt")]
        ),
        "interrupted edit"
    );
    assert_eq!(
        s.git(
            &s.repo,
            &["show", &format!("{stash}^3:.cache/recovery-ignored.txt")]
        ),
        "ignored extra"
    );
    let private_ref = snapshot["private_ref"].as_str().unwrap();
    assert_eq!(s.git(&s.repo, &["rev-parse", private_ref]), stash);
    assert!(
        s.git(
            &result.task_repository,
            &["merge-base", "--is-ancestor", &local, &result.base_commit]
        )
        .is_empty()
    );
    assert!(
        s.git(
            &result.task_repository,
            &["merge-base", "--is-ancestor", &remote, &result.base_commit]
        )
        .is_empty()
    );
}
