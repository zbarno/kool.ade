use super::*;

#[path = "provider_checks/remote_guards.rs"]
mod remote_guards;

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
        None,
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
