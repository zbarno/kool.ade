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
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("changed"), "unexpected error: {error}");
        assert!(!sandbox.root.join("pr-created").exists());
        assert!(sandbox.git(&attacker, &["for-each-ref"]).is_empty());
    }
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
fn required_independent_checks_fail_closed_for_unsupported_git_hosting() {
    let s = Sandbox::new();
    let unsupported_remote = "https://example.test/team/repo.git";
    s.git(
        &s.repo,
        &["config", "remote.origin.url", unsupported_remote],
    );
    s.git(
        &s.repo,
        &[
            "config",
            &format!("url.{}.insteadOf", s.root.join("remote.git").display()),
            unsupported_remote,
        ],
    );
    let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let error = s
        .run_with_publication_policy("complete", calls, PublicationMode::AutoPublish, None, true)
        .unwrap_err();
    assert!(error.to_string().contains("no supported CI provider"));
    let classified = Failure::from_error(&error);
    assert_eq!(classified.kind, FailureKind::ExternalPrerequisite);
    assert_eq!(classified.recovery, RecoveryDisposition::UserAction);
    assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
    let state_dir = state_dir(&s.repo, &s.ticket).unwrap();
    let state = read_state_file(&state_dir.join("state.json")).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    let check = state.independent_check.unwrap();
    assert_eq!(check.status, IndependentCheckStatus::Unavailable);
    assert!(check.detail.unwrap().contains("does not support"));
    assert!(state_dir.join("verified-report.json").exists());
}

#[test]
fn independent_check_gate_pushes_only_the_candidate_and_records_the_exact_result() {
    for passes in [true, false] {
        let s = Sandbox::new();
        let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut state = s.run("complete", calls).unwrap();
        assert!(state.task_repository.exists());
        let commit = state.verified_head.clone().unwrap();
        let candidate_ref = checks::candidate_ref("fixture", &commit);
        let worktree = state.task_repository.clone();
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let (progress, _updates) = mpsc::channel();
        let runner = Runner {
            gh: s.gh.to_string_lossy().into_owned(),
            runtime_config_source: None,
            deadline: Instant::now() + Duration::from_secs(60),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        };
        let outcome = checks_gate::wait_with_provider(
            checks_gate::CheckRequest {
                provider: &ImmediateCheck(passes),
                repository: "github.com/fixture/repo",
                candidate_ref: &candidate_ref,
                repository_path: &worktree,
                commit: &commit,
            },
            &dir,
            &mut state,
            &runner,
            None,
        );
        if passes {
            assert!(
                outcome.is_ok(),
                "independent check detail: {:?}",
                state.independent_check
            );
            outcome.unwrap();
            assert_eq!(state.status, ImplementationStatus::Publishing);
            assert_eq!(
                state.independent_check.as_ref().unwrap().status,
                IndependentCheckStatus::Passed
            );
        } else {
            let error = outcome.unwrap_err();
            assert!(error.to_string().contains("Project checks failed"));
            let classified = Failure::from_error(&error);
            assert_eq!(classified.kind, FailureKind::Verification);
            assert_eq!(classified.recovery, RecoveryDisposition::ExplicitResume);
            assert_eq!(
                state.independent_check.as_ref().unwrap().status,
                IndependentCheckStatus::Failed
            );
        }
        assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
        assert!(
            s.git(&s.repo, &["ls-remote", "origin", &candidate_ref])
                .contains(&commit),
            "the exact integration commit should be on the temporary checks ref"
        );
    }
}

#[test]
fn disabling_auto_publish_during_successful_checks_keeps_default_branch_unchanged() {
    let s = Sandbox::new();
    let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut state = s.run("complete", calls).unwrap();
    let commit = state.verified_head.clone().unwrap();
    let candidate_ref = checks::candidate_ref("fixture", &commit);
    let worktree = state.task_repository.clone();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: s.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    let gate = Arc::new(AtomicBool::new(true));
    checks_gate::wait_with_provider(
        checks_gate::CheckRequest {
            provider: &DisablingCheck(gate.clone()),
            repository: "github.com/fixture/repo",
            candidate_ref: &candidate_ref,
            repository_path: &worktree,
            commit: &commit,
        },
        &dir,
        &mut state,
        &runner,
        Some(&gate),
    )
    .unwrap();

    assert!(!gate.load(Ordering::SeqCst));
    assert_eq!(state.status, ImplementationStatus::AwaitingApproval);
    assert_eq!(
        state.independent_check.as_ref().unwrap().status,
        IndependentCheckStatus::Passed
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
    assert!(
        s.git(&s.repo, &["ls-remote", "origin", &candidate_ref])
            .contains(&commit)
    );
}
