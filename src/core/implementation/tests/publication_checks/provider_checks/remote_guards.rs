use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;

fn publication_runner(sandbox: &Sandbox) -> Runner {
    let (progress, _updates) = mpsc::channel();
    Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    }
}

#[test]
fn task_clone_remote_changes_refuse_pull_request_publication() {
    for key in ["remote.origin.url", "remote.origin.pushurl"] {
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
        let attacker = sandbox.root.join("attacker.git");
        sandbox.git(
            &sandbox.root,
            &["init", "--bare", "-q", attacker.to_str().unwrap()],
        );
        sandbox.git(
            &state.task_repository,
            &["config", key, attacker.to_str().unwrap()],
        );
        let error = publication::create_pull_request(
            &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
            &mut state,
            &publication_runner(&sandbox),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("changed"), "unexpected error: {error}");
        assert!(!sandbox.root.join("pr-created").exists());
        assert!(sandbox.git(&attacker, &["for-each-ref"]).is_empty());
    }
}

#[test]
fn task_clone_with_separate_push_repository_refuses_pr_before_publication() {
    let sandbox = Sandbox::new();
    let push_repository = sandbox.root.join("separate-push.git");
    sandbox.git(
        &sandbox.root,
        &["init", "--bare", "-q", push_repository.to_str().unwrap()],
    );
    sandbox.git(
        &sandbox.repo,
        &[
            "config",
            "remote.origin.pushurl",
            push_repository.to_str().unwrap(),
        ],
    );
    let mut state = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();
    sandbox.git(
        &state.task_repository,
        &[
            "config",
            "koolade.pushIdentityUrl",
            "https://github.com/fixture/repo.git",
        ],
    );

    let error = publication::create_pull_request(
        &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
        &mut state,
        &publication_runner(&sandbox),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("configured and effective push destinations"),
        "unexpected error: {error}"
    );
    assert!(!sandbox.root.join("pr-created").exists());
    assert!(sandbox.git(&push_repository, &["for-each-ref"]).is_empty());
}

#[test]
fn task_clone_url_rewrite_cannot_redirect_the_trusted_publication_push() {
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
    let cache = RepositoryCache::from_saved_state(&state, &cleanup_runner()).unwrap();
    let trusted_push = cache.push_url.as_deref().unwrap();
    let attacker = sandbox.root.join("attacker.git");
    sandbox.git(
        &sandbox.root,
        &["init", "--bare", "-q", attacker.to_str().unwrap()],
    );
    sandbox.git(
        &state.task_repository,
        &[
            "config",
            &format!("url.{}.insteadOf", attacker.display()),
            trusted_push,
        ],
    );

    publication::create_pull_request(
        &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
        &mut state,
        &publication_runner(&sandbox),
        None,
    )
    .unwrap();

    let branch_ref = format!("refs/heads/{}", state.branch);
    assert!(
        sandbox
            .git(&attacker, &["for-each-ref", &branch_ref])
            .is_empty()
    );
    assert!(
        sandbox
            .git(
                &sandbox.root.join("remote.git"),
                &["for-each-ref", &branch_ref]
            )
            .contains(&state.branch)
    );
    assert!(sandbox.root.join("pr-created").exists());
}

#[test]
fn required_independent_checks_reject_redirected_push_destinations() {
    let sandbox = Sandbox::new();
    let unsupported_remote = "https://example.test/team/repo.git";
    sandbox.git(
        &sandbox.repo,
        &["config", "remote.origin.url", unsupported_remote],
    );
    sandbox.git(
        &sandbox.repo,
        &[
            "config",
            &format!(
                "url.{}.insteadOf",
                sandbox.root.join("remote.git").display()
            ),
            unsupported_remote,
        ],
    );
    let remote_main = sandbox.git(&sandbox.repo, &["rev-parse", "origin/main"]);
    let error = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::AutoPublish,
            None,
            true,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("configured and effective push destinations"),
        "unexpected error: {error}"
    );
    let classified = Failure::from_error(&error);
    assert_eq!(classified.kind, FailureKind::RemoteDiverged);
    assert_eq!(classified.recovery, RecoveryDisposition::UserAction);
    assert_eq!(
        sandbox.git(&sandbox.repo, &["rev-parse", "origin/main"]),
        remote_main
    );
    let state_dir = state_dir(&sandbox.repo, &sandbox.ticket).unwrap();
    let state = read_state_file(&state_dir.join("state.json")).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert!(state.independent_check.is_none());
    assert!(state_dir.join("verified-report.json").exists());
}

#[test]
fn required_independent_checks_fail_closed_for_unsupported_git_hosting() {
    let sandbox = Sandbox::new();
    sandbox.git(
        &sandbox.repo,
        &[
            "config",
            "remote.origin.url",
            "ssh://git@example.test/team/repo.git",
        ],
    );
    let error = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::AutoPublish,
            None,
            true,
        )
        .unwrap_err();

    assert!(error.to_string().contains("no supported CI provider"));
    let classified = Failure::from_error(&error);
    assert_eq!(classified.kind, FailureKind::ExternalPrerequisite);
    assert_eq!(classified.recovery, RecoveryDisposition::UserAction);
    let state_dir = state_dir(&sandbox.repo, &sandbox.ticket).unwrap();
    let state = read_state_file(&state_dir.join("state.json")).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    let check = state.independent_check.unwrap();
    assert_eq!(check.status, IndependentCheckStatus::Unavailable);
    assert!(check.detail.unwrap().contains("does not support"));
}

#[test]
fn task_clone_with_separate_push_repository_refuses_independent_checks() {
    let sandbox = Sandbox::new();
    let push_repository = sandbox.root.join("separate-checks-push.git");
    sandbox.git(
        &sandbox.root,
        &["init", "--bare", "-q", push_repository.to_str().unwrap()],
    );
    sandbox.git(
        &sandbox.repo,
        &[
            "config",
            "remote.origin.pushurl",
            push_repository.to_str().unwrap(),
        ],
    );

    let error = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::AutoPublish,
            None,
            true,
        )
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("configured and effective push destinations"),
        "unexpected error: {error}"
    );
    assert!(sandbox.git(&push_repository, &["for-each-ref"]).is_empty());
    assert!(!sandbox.root.join("pr-created").exists());
}
