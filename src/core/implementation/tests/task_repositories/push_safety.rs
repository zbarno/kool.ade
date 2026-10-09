use super::*;

#[test]
fn stale_destination_rejects_a_non_fast_forward_task_push() {
    let sandbox = Sandbox::new();
    let state = run_on_worker(&sandbox, &sandbox.ticket).unwrap();
    let candidate = sandbox.git(&state.task_repository, &["rev-parse", "HEAD"]);
    let remote_head = sandbox.advance_remote();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: Some(sandbox.repo.clone()),
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    let cache = RepositoryCache::from_saved_state(&state, &runner).unwrap();

    let error = cache
        .push_commit(
            &state.task_repository,
            &candidate,
            "refs/heads/main",
            &runner,
        )
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("non-fast-forward") || error.contains("fetch first"),
        "{error}"
    );
    assert_eq!(
        sandbox.git(
            &sandbox.root.join("remote.git"),
            &["rev-parse", "refs/heads/main"]
        ),
        remote_head
    );
}
