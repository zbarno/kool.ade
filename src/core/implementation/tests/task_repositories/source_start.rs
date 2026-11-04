use super::*;

#[test]
fn moved_source_ref_does_not_change_the_approved_task_starting_commit() {
    let sandbox = Sandbox::new();
    let source = "topic/source-start";
    sandbox.git(&sandbox.repo, &["checkout", "-b", source]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", source]);
    fs::write(sandbox.repo.join("local-only.txt"), "local source\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "local-only.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "local source update"]);
    let local_commit = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    super::super::sandbox::approve_ticket_source(&sandbox, source, "main");

    let peer = sandbox.root.join("peer-source-start");
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

        authorization_verifier: None,
    };
    execution::persist_source_plan_before_task_state(
        &sandbox.planning_store.root,
        &sandbox.repo,
        &sandbox.ticket,
        &runner,
    )
    .unwrap();

    let state = execution::prepare_task_repository_before_reconciliation(
        &sandbox.planning_store.root,
        &sandbox.repo,
        &sandbox.ticket,
        &runner,
    )
    .unwrap();
    assert_eq!(state.source_commit.as_deref(), Some(local_commit.as_str()));
    assert_eq!(state.base_commit, local_commit);
    assert_eq!(
        sandbox.git(&state.task_repository, &["rev-parse", "HEAD"]),
        local_commit,
        "the independent clone starts at the exact source SHA before reconciliation"
    );
    assert!(state.task_repository.join("local-only.txt").is_file());
    assert!(!state.task_repository.join("remote-only.txt").exists());

    let state = run_on_worker(&sandbox, &sandbox.ticket).unwrap();
    assert!(state.task_repository.join("local-only.txt").is_file());
    assert!(
        state.task_repository.join("remote-only.txt").is_file(),
        "the approved local commit remains pinned while reconciliation also preserves the advertised remote history"
    );
    assert!(state.task_repository.join("implemented.txt").is_file());
}
