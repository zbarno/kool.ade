use super::*;

// Production boundary: the PR preflight refreshes a real cached Git remote and
// checks the controlled task clone without rewriting its reviewed commit. A
// head-comparison unit test cannot prove that the Git integration controller
// preserves identity or refuses local edits. Local Git/SSH/provider fixtures;
// one verified task, moderate setup, no Docker or external network.
#[test]
fn current_reviewed_base_preserves_head_and_rejects_unverified_clone_edits() {
    let sandbox = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut state = sandbox
        .run_with_publication_policy(
            "complete",
            calls.clone(),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();
    let original_head = state.verified_head.clone();
    let original_branch = state.branch.clone();
    let original_clone = state.task_repository.clone();
    let dir = state_dir(&sandbox.repo, &sandbox.ticket).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    let policy = ExecutionPolicy {
        publication_mode: PublicationMode::CreatePullRequest,
        auto_publish_gate: None,
        require_independent_checks: false,
        user_context: None,
        accrual: None,
        claim_lease: None,
    };
    integration::prepare_reviewed_for_pull_request(
        &sandbox.repo,
        &dir,
        &mut state,
        &Fixture {
            mode: "fail",
            calls: calls.clone(),
        },
        &runner,
        &policy,
    )
    .unwrap();
    assert_eq!(state.verified_head, original_head);
    assert_eq!(state.branch, original_branch);
    assert_eq!(state.task_repository, original_clone);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let unverified = state.task_repository.join("unverified.txt");
    fs::write(&unverified, "unverified source edit").unwrap();
    assert!(
        integration::prepare_reviewed_for_pull_request(
            &sandbox.repo,
            &dir,
            &mut state,
            &Fixture {
                mode: "fail",
                calls
            },
            &runner,
            &policy,
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(&unverified).unwrap(),
        "unverified source edit"
    );
    assert_eq!(state.verified_head, original_head);
    assert!(!sandbox.root.join("pr-created").exists());
    std::fs::remove_file(unverified).unwrap();
    sandbox.advance_remote();
    let error = publication::create_pull_request(&dir, &mut state, &runner, None).unwrap_err();
    assert!(error.to_string().contains("Destination branch moved"));
    assert!(!sandbox.root.join("pr-created").exists());
    assert!(
        sandbox
            .git(
                &sandbox.root.join("remote.git"),
                &["for-each-ref", &format!("refs/heads/{}", state.branch)]
            )
            .is_empty()
    );
}
