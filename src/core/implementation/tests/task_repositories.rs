use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;

#[path = "task_repositories/source_start.rs"]
mod source_start;

fn run_on_worker(sandbox: &Sandbox, ticket: &str) -> anyhow::Result<Implementation> {
    let (progress, _updates) = mpsc::channel();
    run_with_project_options(
        &sandbox.repo,
        &sandbox.repo,
        ticket,
        RunOptions {
            harness: &Fixture {
                mode: "complete",
                calls: Arc::new(AtomicUsize::new(0)),
            },
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode: PublicationMode::HoldForReview,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
        },
    )
}

#[test]
fn dirty_user_checkout_on_another_branch_starts_from_the_exact_local_commit() {
    let sandbox = Sandbox::new();
    sandbox.git(&sandbox.repo, &["checkout", "-b", "human/working"]);
    fs::write(sandbox.repo.join("tracked.txt"), "committed source\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "tracked.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "local source commit"]);
    let source_commit = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    fs::write(sandbox.repo.join("tracked.txt"), "uncommitted user edit\n").unwrap();
    fs::write(sandbox.repo.join("user-only.txt"), "untracked user file\n").unwrap();
    let user_status = sandbox.git(&sandbox.repo, &["status", "--porcelain"]);

    let state = run_on_worker(&sandbox, &sandbox.ticket).unwrap();

    assert_eq!(state.task_repository_kind, TaskRepositoryKind::Clone);
    assert_eq!(
        sandbox.git(&state.task_repository, &["config", "--local", "user.name"]),
        "Fixture"
    );
    assert_eq!(
        sandbox.git(&state.task_repository, &["config", "--local", "user.email"]),
        "fixture@example.test"
    );
    assert_eq!(
        sandbox.git(
            &state.task_repository,
            &["config", "--local", "user.useConfigOnly"]
        ),
        "true"
    );
    assert_eq!(state.source_ref.as_deref(), Some("human/working"));
    assert_eq!(state.source_commit.as_deref(), Some(source_commit.as_str()));
    assert_eq!(state.base_commit, source_commit);
    assert_eq!(state.status, ImplementationStatus::AwaitingApproval);
    assert!(state.task_repository.join(".git").is_dir());
    assert!(
        !state
            .task_repository
            .join(".git/objects/info/alternates")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(state.task_repository.join("tracked.txt")).unwrap(),
        "committed source\n"
    );
    assert!(!state.task_repository.join("user-only.txt").exists());
    assert_eq!(
        sandbox.git(&sandbox.repo, &["symbolic-ref", "--short", "HEAD"]),
        "human/working"
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["status", "--porcelain"]),
        user_status
    );
    assert_eq!(
        fs::read_to_string(sandbox.repo.join("tracked.txt")).unwrap(),
        "uncommitted user edit\n"
    );
    assert_eq!(
        fs::read_to_string(sandbox.repo.join("user-only.txt")).unwrap(),
        "untracked user file\n"
    );
    assert!(
        !sandbox
            .git(&sandbox.repo, &["worktree", "list", "--porcelain"])
            .contains(state.task_repository.to_str().unwrap())
    );
}

#[test]
fn clone_reconciliation_plan_resumes_before_task_state_was_saved() {
    let sandbox = Sandbox::new();
    let source = "topic/recovery";
    sandbox.git(&sandbox.repo, &["checkout", "-b", source]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", source]);
    fs::write(sandbox.repo.join("local-only.txt"), "local source\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "local-only.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "local source update"]);
    let local_commit = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);

    let peer = sandbox.root.join("peer-source");
    sandbox.git(
        &sandbox.root,
        &[
            "clone",
            "-q",
            "--branch",
            source,
            sandbox.root.join("remote.git").to_str().unwrap(),
            peer.to_str().unwrap(),
        ],
    );
    sandbox.git(&peer, &["config", "user.name", "Fixture"]);
    sandbox.git(&peer, &["config", "user.email", "fixture@example.test"]);
    fs::write(peer.join("remote-only.txt"), "remote source\n").unwrap();
    sandbox.git(&peer, &["add", "remote-only.txt"]);
    sandbox.git(&peer, &["commit", "-qm", "remote source update"]);
    sandbox.git(&peer, &["push", "-q", "origin", source]);

    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: Some(sandbox.repo.clone()),
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    execution::persist_source_plan_before_task_state(
        &sandbox.repo,
        &sandbox.repo,
        &sandbox.ticket,
        &runner,
    )
    .unwrap();

    let (_, task_uid, _) = read_ticket_and_identity(&sandbox.repo, &sandbox.ticket).unwrap();
    let dir = state_dir_for_task(&sandbox.repo, &sandbox.ticket, task_uid.as_deref()).unwrap();
    assert!(!dir.join("state.json").exists());
    let plan = crate::core::implementation::initial_reconciliation::load_plan(&dir)
        .unwrap()
        .unwrap();
    assert_eq!(plan.local_commit, local_commit);
    assert_eq!(plan.clone_repository.as_ref().unwrap().source_ref, source);

    sandbox.git(&sandbox.repo, &["checkout", "main"]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "--delete", source]);
    sandbox.git(&sandbox.repo, &["branch", "-D", source]);
    sandbox.git(&sandbox.repo, &["fetch", "--prune", "origin"]);
    sandbox.git(
        &sandbox.repo,
        &["reflog", "expire", "--expire=now", "--all"],
    );
    sandbox.git(&sandbox.repo, &["gc", "--prune=now"]);
    let source_object = std::process::Command::new("git")
        .args(["cat-file", "-e", &format!("{local_commit}^{{commit}}")])
        .current_dir(&sandbox.repo)
        .status()
        .unwrap();
    assert!(
        !source_object.success(),
        "source commit left the user checkout"
    );

    let state = run_on_worker(&sandbox, &sandbox.ticket).unwrap();
    assert_eq!(state.task_repository_kind, TaskRepositoryKind::Clone);
    assert_eq!(state.source_ref.as_deref(), Some(source));
    assert_eq!(state.source_commit.as_deref(), Some(local_commit.as_str()));
    assert!(state.task_repository.join("local-only.txt").is_file());
    assert!(state.task_repository.join("remote-only.txt").is_file());
}

#[test]
fn concurrent_tasks_get_independent_clones_and_branches() {
    let sandbox = Sandbox::new();
    let second_ticket = ".koolade-packet/planning/tasks/feature/002-independent-task.md";
    fs::write(
        sandbox.repo.join(second_ticket),
        "# Independent task\n\n## Acceptance criteria\n\n- File contains implemented.\n",
    )
    .unwrap();
    sandbox.git(&sandbox.repo, &["add", second_ticket]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "add independent task"]);

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| run_on_worker(&sandbox, &sandbox.ticket));
        let second = scope.spawn(|| run_on_worker(&sandbox, second_ticket));
        (first.join().unwrap(), second.join().unwrap())
    });
    let first = first.unwrap();
    let second = second.unwrap();

    assert_ne!(first.task_repository, second.task_repository);
    assert_ne!(first.branch, second.branch);
    assert_ne!(
        common(&first.task_repository).unwrap(),
        common(&second.task_repository).unwrap()
    );
    assert!(first.task_repository.join(".git").is_dir());
    assert!(second.task_repository.join(".git").is_dir());
    assert_eq!(
        first
            .task_repository_commits
            .get(&first.task_repository.to_string_lossy().into_owned()),
        first.verified_head.as_ref()
    );

    fs::write(first.task_repository.join("isolated.txt"), "first clone\n").unwrap();
    sandbox.git(&first.task_repository, &["add", "isolated.txt"]);
    sandbox.git(
        &first.task_repository,
        &["commit", "-qm", "change first task clone"],
    );
    assert!(!second.task_repository.join("isolated.txt").exists());
    assert!(!sandbox.repo.join("isolated.txt").exists());
}

#[test]
fn repository_caches_are_isolated_by_the_effective_push_endpoint() {
    let sandbox = Sandbox::new();
    let first = run_on_worker(&sandbox, &sandbox.ticket).unwrap();
    let alternate_remote = sandbox.root.join("alternate.git");
    sandbox.git(
        &sandbox.root,
        &["init", "--bare", "-q", alternate_remote.to_str().unwrap()],
    );
    sandbox.git(
        &sandbox.repo,
        &[
            "config",
            "remote.origin.pushurl",
            alternate_remote.to_str().unwrap(),
        ],
    );
    let second_ticket = ".koolade-packet/planning/tasks/feature/002-distinct-push-endpoint.md";
    fs::write(
        sandbox.repo.join(second_ticket),
        "# Distinct push endpoint\n\n## Acceptance criteria\n\n- File contains implemented.\n",
    )
    .unwrap();
    sandbox.git(&sandbox.repo, &["add", second_ticket]);
    sandbox.git(
        &sandbox.repo,
        &["commit", "-qm", "add endpoint isolation task"],
    );
    let second = run_on_worker(&sandbox, second_ticket).unwrap();

    let runner = cleanup_runner();
    let first_cache = RepositoryCache::from_saved_state(&first, &runner).unwrap();
    let second_cache = RepositoryCache::from_saved_state(&second, &runner).unwrap();
    assert_ne!(first_cache.path, second_cache.path);
    assert_ne!(first_cache.push_url, second_cache.push_url);
    assert_eq!(
        second_cache.push_url.as_deref(),
        Some(alternate_remote.to_str().unwrap())
    );
}
