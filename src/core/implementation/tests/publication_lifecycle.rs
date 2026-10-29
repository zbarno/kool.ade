use super::*;
use crate::core::implementation::publication::lifecycle::tests::fixture::Fixture as LifecycleFixture;
use crate::core::implementation::publication::{
    LifecyclePublicationGate, create_pull_request, finish_lifecycle_publication,
    prepare_lifecycle_publication, preserve_done_after_failure,
};
use crate::core::lifecycle::{LifecycleStore, PullRequestPreparationStatus};

#[path = "publication_lifecycle/evolve.rs"]
mod evolve;

/// Production boundary: authenticated app Pursue, persisted Evolve run,
/// explicit publication authorization, Git push, provider PR creation/readback,
/// and app projection after a lost response and store/app reconstruction. The
/// invariant is that an unknown provider outcome stays PrPending and recovery
/// opens the same PR once. Unit tests cannot prove this durable cross-module
/// handoff. Plan/review/QA/spec receipts remain synthetic at this boundary.
/// Uses a Python-only source tree without Cargo or npm manifests, a temporary
/// task clone, local SSH remote, fake `gh`, and headless app fixture; no external
/// network, Docker, model, or browser. This verifies publication remains a Git
/// and provider operation independent of the project's build system. Setup
/// cost is a temporary repository, lifecycle store, and provider-fixture processes.
#[test]
fn authorized_lifecycle_publication_recovers_provider_timeout_without_duplicate_pr() {
    let sandbox = Sandbox::new_with_initial_files(&[(
        "src/server.py",
        "def status():\n    return 'ready'\n",
    )]);
    assert!(!sandbox.repo.join("Cargo.toml").exists());
    assert!(!sandbox.repo.join("package.json").exists());
    let product_pr_url = "https://github.com/zbarno/kool.ade/pull/999999";
    let gh_script = fs::read_to_string(&sandbox.gh).unwrap();
    let product_gh_script =
        gh_script.replace("https://github.com/fixture/repo/pull/1", product_pr_url);
    assert_ne!(product_gh_script, gh_script, "fake provider has a PR URL");
    fs::write(&sandbox.gh, product_gh_script).unwrap();
    let mut state = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();
    assert!(!state.task_repository.join("Cargo.toml").exists());
    assert!(!state.task_repository.join("package.json").exists());

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
    // The default destination is held in `base`; `destination_branch` is only
    // populated when the user explicitly chose one.
    state.destination_branch = None;

    let (mut pursuit_app, planning, product_root) =
        crate::app::root::self_improvement_test_support::app_fixture();
    let (opportunity_uid, opportunity_revision) =
        evolve::save_product_proposal(&planning, &product_root, &state.base_commit);
    crate::app::root::self_improvement_test_support::refresh_planning_state(
        &mut pursuit_app,
        &planning,
    );
    let identity_gh = sandbox.root.join("identity-gh");
    fs::write(
        &identity_gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&identity_gh, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let pursue_message = crate::app::root::self_improvement_test_support::decide_product_pursuit(
        &mut pursuit_app,
        &opportunity_uid,
        opportunity_revision,
        &identity_gh,
    );
    assert!(
        pursue_message
            .as_deref()
            .is_some_and(|message| message.contains("self-improvement run were added")),
        "app Pursue should create a self-improvement run, got: {pursue_message:?}"
    );
    let opportunities = crate::core::opportunity::OpportunityStore::new(&planning);
    let decision = opportunities
        .decision_for_opportunity(&opportunity_uid)
        .unwrap()
        .expect("the app Pursue action persists its decision");
    assert_eq!(decision.user_id, "github:987654321");
    let run_uid =
        crate::core::self_improvement::SelfImprovementStore::run_uid_for_decision(&decision.uid)
            .unwrap();
    let pursued_run = crate::core::self_improvement::SelfImprovementStore::new(&planning)
        .load(&run_uid)
        .unwrap();
    assert_eq!(
        pursued_run.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Planning
    );
    assert!(pursued_run.clone_uid.is_none());

    let fixture = LifecycleFixture::with_store(
        planning.clone(),
        product_root,
        pursued_run.feature_uid,
        &reviewed_head,
        state
            .task_uid
            .clone()
            .expect("fixture task has lifecycle binding"),
        true,
    );
    evolve::seed_pr_pending_run(&fixture, &run_uid, &state.base_commit);
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
    let mut interrupted_app = crate::app::root::self_improvement_test_support::app_with_publication(
        &store,
        interrupted_state.clone(),
    );
    crate::app::root::self_improvement_test_support::synchronize_run(
        &mut interrupted_app,
        &store,
        &run_uid,
    )
    .unwrap();
    let interrupted_run = crate::core::self_improvement::SelfImprovementStore::new(&store)
        .load(&run_uid)
        .unwrap();
    assert_eq!(
        interrupted_run.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::PrPending,
        "an unknown provider outcome must not project an unverified PR URL"
    );
    assert!(interrupted_run.pr_url.is_none());
    drop(interrupted_app);

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

    assert_eq!(recovered_state.pr_url.as_deref(), Some(product_pr_url));
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

    let mut recovered_app = crate::app::root::self_improvement_test_support::app_with_publication(
        &reopened_store,
        recovered_state.clone(),
    );
    crate::app::root::self_improvement_test_support::synchronize_run(
        &mut recovered_app,
        &reopened_store,
        &run_uid,
    )
    .unwrap();
    let runs = crate::core::self_improvement::SelfImprovementStore::new(&reopened_store);
    let projected = runs.load(&run_uid).unwrap();
    assert_eq!(
        projected.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::PrOpened
    );
    assert_eq!(
        projected.pr_url.as_deref(),
        recovered_state.pr_url.as_deref()
    );
    crate::app::root::self_improvement_test_support::synchronize_run(
        &mut recovered_app,
        &reopened_store,
        &run_uid,
    )
    .unwrap();
    assert_eq!(
        runs.load(&run_uid).unwrap().revision,
        projected.revision,
        "repeating projection must not duplicate PrOpened"
    );
}
