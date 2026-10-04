use super::*;

#[test]
fn verified_disjoint_baseline_is_reused_by_followup_tasks() {
    let sandbox = Sandbox::new();
    let second_ticket = ".koolade-packet/planning/tasks/feature/002-follow-up-ticket.md";
    fs::write(
        sandbox.repo.join(second_ticket),
        "# Follow-up implementation\n\n## Acceptance criteria\n\n- File contains implemented.\n",
    )
    .unwrap();
    sandbox.git(&sandbox.repo, &["add", second_ticket]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "add follow-up ticket"]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "main"]);
    let remote = sandbox.advance_remote();
    fs::write(sandbox.repo.join("local.txt"), "local change\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "local.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "local change"]);
    let local = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);

    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };
    let first = run_with_agent(&sandbox, &agent, None).unwrap();

    let worktree_root = sandbox
        .repo
        .parent()
        .unwrap()
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(
            &sandbox.repo.canonicalize().unwrap(),
        ));
    let task_key = crate::core::implementation::key_for_ticket(second_ticket);
    let second_worktree = worktree_root.join(&task_key);
    fs::create_dir_all(&worktree_root).unwrap();
    let branch = format!("koolade/{task_key}");
    sandbox.git(
        &sandbox.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            second_worktree.to_str().unwrap(),
            &remote,
        ],
    );
    sandbox.git(
        &second_worktree,
        &["merge", "--no-ff", "--no-commit", "--no-edit", &local],
    );
    assert!(
        std::process::Command::new("git")
            .args(["rev-parse", "--verify", "MERGE_HEAD"])
            .current_dir(&second_worktree)
            .output()
            .unwrap()
            .status
            .success()
    );

    let (progress, updates) = mpsc::channel();
    let second =
        run_with_agent_for_ticket(&sandbox, second_ticket, &agent, None, progress).unwrap();
    let activities = updates
        .try_iter()
        .filter_map(|update| update.activity)
        .collect::<Vec<_>>();

    assert_eq!(first.base_commit, second.base_commit);
    assert_eq!(second.worktree, second_worktree);
    assert!(
        sandbox
            .git(&second.worktree, &["status", "--porcelain"])
            .is_empty()
    );
    assert!(
        activities
            .iter()
            .any(|activity| activity.contains("Reused the verified shared baseline"))
    );
    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        sandbox
            .git(
                &sandbox.repo,
                &[
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/koolade-reconciliations/shared"
                ]
            )
            .lines()
            .count(),
        1
    );
}

#[test]
fn fast_forward_hook_edits_are_not_accepted_as_a_cached_baseline() {
    let sandbox = Sandbox::new();
    let second_ticket = ".koolade-packet/planning/tasks/feature/002-follow-up-ticket.md";
    fs::write(
        sandbox.repo.join(second_ticket),
        "# Follow-up implementation\n\n## Acceptance criteria\n\n- File contains implemented.\n",
    )
    .unwrap();
    sandbox.git(&sandbox.repo, &["add", second_ticket]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "add follow-up ticket"]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "main"]);
    sandbox.advance_remote();
    fs::write(sandbox.repo.join("local.txt"), "local change\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "local.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "local change"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = ReconcilingAgent {
        calls: calls.clone(),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };
    run_with_agent(&sandbox, &agent, None).unwrap();

    let hook_dir = sandbox
        .repo
        .join(sandbox.git(&sandbox.repo, &["rev-parse", "--git-path", "hooks"]));
    fs::create_dir_all(&hook_dir).unwrap();
    let hook = hook_dir.join("post-merge");
    fs::write(&hook, "#!/bin/sh\nprintf 'hook changed\n' > upstream.txt\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let (progress, _updates) = mpsc::channel();

    let error = run_with_agent_for_ticket(&sandbox, second_ticket, &agent, None, progress)
        .unwrap_err()
        .to_string();

    assert!(error.contains("left worktree changes"), "{error}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state = load(&sandbox.repo, second_ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert_eq!(
        fs::read_to_string(state.worktree.join("upstream.txt")).unwrap(),
        "hook changed\n"
    );
}
