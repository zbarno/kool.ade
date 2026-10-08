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
    sandbox.advance_remote();
    fs::write(sandbox.repo.join("local.txt"), "local change\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "local.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "local change"]);
    sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);

    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };
    let first = run_with_agent(&sandbox, &agent, None).unwrap();

    let (progress, updates) = mpsc::channel();
    let second =
        run_with_agent_for_ticket(&sandbox, second_ticket, &agent, None, progress).unwrap();
    let activities = updates
        .try_iter()
        .filter_map(|update| update.activity)
        .collect::<Vec<_>>();

    assert_eq!(first.base_commit, second.base_commit);
    assert_eq!(second.task_repository_kind, TaskRepositoryKind::Clone);
    assert_ne!(second.task_repository, first.task_repository);
    assert!(second.task_repository.join(".git").is_dir());
    assert!(
        sandbox
            .git(&second.task_repository, &["status", "--porcelain"])
            .is_empty()
    );
    assert!(
        activities
            .iter()
            .any(|activity| activity.contains("Reused the verified shared baseline"))
    );
    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    let task_refs = format!(
        "refs/koolade-reconciliations/{}",
        crate::core::implementation::key_for_ticket(&sandbox.ticket)
    );
    assert_eq!(
        sandbox
            .git(
                second.repository_cache.as_deref().unwrap(),
                &["for-each-ref", "--format=%(refname)", &task_refs]
            )
            .lines()
            .count(),
        2
    );
    assert!(
        sandbox
            .git(
                &sandbox.repo,
                &["for-each-ref", "--format=%(refname)", &task_refs]
            )
            .is_empty()
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
    let first = run_with_agent(&sandbox, &agent, None).unwrap();
    let runner = cleanup_runner();
    let cache = crate::core::implementation::repository_cache::RepositoryCache::from_saved_state(
        &first, &runner,
    )
    .unwrap();
    let first_dir = state_dir(&sandbox.repo, &sandbox.ticket).unwrap();
    let plan = crate::core::implementation::initial_reconciliation::load_plan(&first_dir)
        .unwrap()
        .unwrap();
    let source_ref = cache.pin_source(&plan.remote_commit, &runner).unwrap();
    let branch = format!(
        "koolade/{}",
        crate::core::implementation::key_for_ticket(second_ticket)
    );
    let task_repository = crate::core::implementation::task_repository::task_path(
        &sandbox.repo,
        first.repository_id.as_deref().unwrap(),
        second_ticket,
    )
    .unwrap();
    cache
        .create_clone(
            &source_ref,
            &plan.remote_commit,
            &branch,
            &task_repository,
            &crate::core::implementation::repository_cache::read_task_git_identity(&first_dir)
                .unwrap(),
            &runner,
        )
        .unwrap();
    let mut second = first.clone();
    second.ticket = second_ticket.into();
    second.task_repository_allocation_key =
        Some(crate::core::implementation::key_for_ticket(second_ticket));
    second.ticket_text = fs::read_to_string(sandbox.repo.join(second_ticket)).unwrap();
    second.branch = branch;
    second.source_ref = Some(source_ref);
    second.source_commit = Some(plan.remote_commit.clone());
    second.task_repository = task_repository.clone();
    second.task_repository_ready = true;
    second.task_repositories = vec![task_repository.clone()];
    second.task_repository_commits.clear();
    second.base_commit = plan.remote_commit.clone();
    second.status = ImplementationStatus::Preparing;

    let hook_dir = task_repository.join(".git/hooks");
    fs::create_dir_all(&hook_dir).unwrap();
    let hook = hook_dir.join("post-merge");
    fs::write(&hook, "#!/bin/sh\nprintf 'hook changed\n' > upstream.txt\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let cached = crate::core::implementation::initial_reconciliation::cache::load(
        &cache.path,
        &plan,
        &runner,
    )
    .unwrap()
    .unwrap();
    let error = crate::core::implementation::initial_reconciliation::cache::adopt(
        &cache.path,
        &second,
        &plan,
        &cached,
        &runner,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("left task repository changes"), "{error}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(task_repository.join("upstream.txt")).unwrap(),
        "hook changed\n"
    );
}
