use super::*;

#[test]
fn clean_disjoint_merge_with_an_unexpected_head_is_not_auto_accepted() {
    let sandbox = Sandbox::new();
    let remote = sandbox.advance_remote();
    sandbox.git(&sandbox.repo, &["fetch", "-q", "origin", "main"]);
    fs::write(sandbox.repo.join("local.txt"), "local change\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "local.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "local change"]);
    let local = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);

    let task_key = crate::core::implementation::key_for_ticket(&sandbox.ticket);
    let worktree = sandbox
        .repo
        .parent()
        .unwrap()
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(
            &sandbox.repo.canonicalize().unwrap(),
        ))
        .join(&task_key);
    fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    let branch = format!("koolade/{task_key}");
    sandbox.git(
        &sandbox.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_str().unwrap(),
            &remote,
        ],
    );
    fs::write(worktree.join("unplanned.txt"), "preserved extra commit\n").unwrap();
    sandbox.git(&worktree, &["add", "unplanned.txt"]);
    sandbox.git(
        &worktree,
        &["commit", "-qm", "unexpected task branch commit"],
    );
    sandbox.git(
        &worktree,
        &["merge", "--no-ff", "--no-commit", "--no-edit", &local],
    );
    let common = sandbox.git(&sandbox.repo, &["merge-base", &local, &remote]);
    super::save_legacy_reconciliation_state(
        &sandbox,
        &sandbox.ticket,
        &worktree,
        "main",
        &local,
        &remote,
        &common,
    );

    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };
    let (progress, _updates) = mpsc::channel();
    let error = run_with_agent_for_ticket(&sandbox, &sandbox.ticket, &agent, None, progress)
        .unwrap_err()
        .to_string();

    assert!(error.contains("unexpected merge parents"), "{error}");
    assert_eq!(agent.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fs::read_to_string(worktree.join("unplanned.txt")).unwrap(),
        "preserved extra commit\n"
    );
    assert!(worktree.join("local.txt").exists());
    assert!(
        std::process::Command::new("git")
            .args(["rev-parse", "--verify", "MERGE_HEAD"])
            .current_dir(&worktree)
            .output()
            .unwrap()
            .status
            .success()
    );
}
