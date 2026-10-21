use super::*;
use crate::core::implementation::publication::lifecycle::tests::fixture::Fixture as LifecycleFixture;
use crate::core::implementation::publication::{
    LifecyclePublicationGate, create_pull_request, finish_lifecycle_publication,
    prepare_lifecycle_publication, preserve_done_after_failure,
};
use crate::core::lifecycle::{LifecycleStore, PullRequestPreparationStatus};

/// Production boundary: explicit lifecycle authorization, Git push, provider PR
/// creation/readback, and durable Opened projection across a lost application
/// response. Reducer tests cannot prove the application reuses an unknown
/// provider outcome. Uses a temporary task clone, local SSH remote, fake `gh`,
/// and file-backed lifecycle store; no external network, Docker, or browser.
#[test]
fn authorized_lifecycle_publication_recovers_provider_timeout_without_duplicate_pr() {
    let sandbox = Sandbox::new();
    let mut state = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();

    // Represent a separately completed child commit in the integrated parent
    // branch. The lifecycle review fixture is bound to this final head.
    let child_change = state.task_repository.join("child-subtask.txt");
    fs::write(&child_change, "child implementation\n").unwrap();
    sandbox.git(&state.task_repository, &["add", "child-subtask.txt"]);
    sandbox.git(
        &state.task_repository,
        &["commit", "-qm", "complete child subtask"],
    );
    let reviewed_head = sandbox.git(&state.task_repository, &["rev-parse", "HEAD"]);
    state.verified_head = Some(reviewed_head.clone());
    state.repository_id = Some("synthetic-publication-repository".into());
    state.destination_branch = Some("main".into());

    let fixture = LifecycleFixture::with_subtask_head(
        reviewed_head.len(),
        &reviewed_head,
        state
            .task_uid
            .clone()
            .expect("fixture task has lifecycle binding"),
    );
    let store = fixture.store.clone();
    let lifecycle = LifecycleStore::new(&store);
    let LifecyclePublicationGate::Authorized(_binding) =
        prepare_lifecycle_publication(&store, &state).unwrap()
    else {
        panic!("explicit Create PR action should persist authorization");
    };
    let authorized = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
    assert_eq!(authorized.status, PullRequestPreparationStatus::Publishing);
    assert!(authorized.publication_authorization.is_some());

    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };

    // The provider accepts the PR, then times out before returning a response.
    // The normal failure classifier durably records an unknown remote outcome.
    fs::write(sandbox.root.join("timeout-after-create"), "once\n").unwrap();
    let mut interrupted_state = state.clone();
    let error = create_pull_request(
        &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
        &mut interrupted_state,
        &runner,
        None,
    )
    .unwrap_err();
    assert!(!error.to_string().is_empty());
    assert!(
        preserve_done_after_failure(
            &store,
            &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
            &mut interrupted_state,
            Some(authorized.publication_lease_epoch()),
            &error,
        )
        .unwrap()
    );
    assert_eq!(interrupted_state.status, ImplementationStatus::Completed);
    assert!(
        interrupted_state
            .detail
            .contains("remote outcome may be unknown")
    );
    let needs_attention = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
    assert_eq!(
        needs_attention.status,
        PullRequestPreparationStatus::NeedsAttention
    );
    assert_eq!(
        sandbox.git(
            &sandbox.root,
            &[
                "--git-dir",
                sandbox.root.join("remote.git").to_str().unwrap(),
                "rev-list",
                "--count",
                &format!("{}..refs/heads/{}", state.base_commit, state.branch),
            ],
        ),
        "2",
        "the parent PR branch contains both integrated commits"
    );
    assert_eq!(
        fs::read_to_string(sandbox.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1
    );

    let reopened = fixture.reopen();
    let LifecyclePublicationGate::Authorized(retry_binding) =
        prepare_lifecycle_publication(&reopened, &state).unwrap()
    else {
        panic!("authorized unknown provider outcome should be retryable");
    };
    let mut recovered_state = interrupted_state;
    create_pull_request(
        &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
        &mut recovered_state,
        &runner,
        None,
    )
    .unwrap();
    finish_lifecycle_publication(&reopened, &recovered_state, Some(retry_binding)).unwrap();

    assert_eq!(
        recovered_state.pr_url.as_deref(),
        Some("https://github.com/fixture/repo/pull/1")
    );
    assert_eq!(
        fs::read_to_string(sandbox.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1,
        "retry must reuse the provider PR rather than create another"
    );
    let reopened_store = fixture.reopen();
    let reopened_lifecycle = LifecycleStore::new(&reopened_store);
    let opened = reopened_lifecycle
        .load_pr_preparation(&fixture.work_uid)
        .unwrap();
    assert_eq!(opened.status, PullRequestPreparationStatus::Opened);
    assert_eq!(opened.provider_pr_id, Some(1));
    assert_eq!(opened.head_sha.as_deref(), Some(reviewed_head.as_str()));
    assert_eq!(opened.included_task_uids, authorized.included_task_uids);
    assert_eq!(opened.included_task_uids.len(), 2);
    let child_uid = &opened.included_task_uids[1];
    assert_eq!(
        reopened_lifecycle
            .load_task(child_uid)
            .unwrap()
            .parent_task_uid
            .as_deref(),
        Some(fixture.task.task_uid.as_str())
    );
}
