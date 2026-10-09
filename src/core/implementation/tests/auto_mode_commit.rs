use super::*;

#[test]
fn auto_mode_verifies_and_stops_for_review_without_pushing_or_creating_pr() {
    let s = Sandbox::new();
    let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
    fs::write(s.repo.join("draft.txt"), "preserve this draft").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let run = || {
        let (tx, _rx) = mpsc::channel();
        run_with_options(
            &s.repo,
            &s.ticket,
            &Fixture {
                mode: "complete",
                calls: calls.clone(),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
            s.gh.to_str().unwrap(),
            true,
        )
    };
    let result = run().unwrap();
    assert_eq!(result.status, ImplementationStatus::AwaitingApproval);
    assert!(result.pr_url.is_none());
    assert!(result.task_repository.exists());
    assert!(
        result
            .independent_check
            .as_ref()
            .is_some_and(|check| check.status == IndependentCheckStatus::Passed)
    );
    let remote = s.root.join("remote.git");
    assert_eq!(s.git(&remote, &["rev-parse", "refs/heads/main"]), original);
    assert_eq!(
        s.git(
            &remote,
            &[
                "rev-list",
                "--count",
                &format!("{original}..refs/heads/main")
            ]
        ),
        "0"
    );
    assert!(
        !s.git(&remote, &["ls-tree", "-r", "--name-only", "main"])
            .contains("implemented.txt")
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
    assert_eq!(
        fs::read_to_string(s.repo.join("draft.txt")).unwrap(),
        "preserve this draft"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!s.root.join("pr-created").exists());
}

#[test]
fn resumed_worker_receives_the_submitted_task_decision() {
    struct Capture {
        prompt: Arc<std::sync::Mutex<String>>,
    }
    impl AiHarness for Capture {
        fn label(&self) -> String {
            "capture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("capture".into())
        }
        fn execute(
            &self,
            req: &PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, AppError> {
            *self.prompt.lock().unwrap() = req.prompt_body.clone();
            Fixture {
                mode: "complete",
                calls: Arc::new(AtomicUsize::new(0)),
            }
            .execute(req)
        }
    }
    let s = Sandbox::new();
    let prompt = Arc::new(std::sync::Mutex::new(String::new()));
    let (tx, _rx) = mpsc::channel();
    run_with_project_options(
        &s.repo,
        &s.repo,
        &s.ticket,
        RunOptions {
            harness: &Capture {
                prompt: prompt.clone(),
            },
            cancel: Arc::new(AtomicBool::new(false)),
            progress: tx,
            gh: s.gh.to_str().unwrap(),
            publication_mode: PublicationMode::AutoPublish,
            require_independent_checks: false,
            user_context: Some("I choose option (b): reissue the corrected footprint predicate."),
            auto_publish_gate: None,
            claim_lease: None,
        },
    )
    .unwrap();
    let recorded = prompt.lock().unwrap();
    assert!(recorded.contains("LATEST SUBMITTED USER RESPONSE FOR THIS TASK"));
    assert!(recorded.contains("I choose option (b): reissue the corrected footprint predicate."));
    assert!(recorded.contains("not proof that the ledger was changed"));
}

#[test]
fn auto_mode_starts_on_remote_when_local_history_diverged() {
    let s = Sandbox::new();
    let remote = s.advance_remote();
    fs::write(s.repo.join("local-only.txt"), "local work").unwrap();
    s.git(&s.repo, &["add", "local-only.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local work"]);
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let (tx, _rx) = mpsc::channel();
    let result = run_with_options(
        &s.repo,
        &s.ticket,
        &Fixture {
            mode: "complete",
            calls: Arc::new(AtomicUsize::new(0)),
        },
        Arc::new(AtomicBool::new(false)),
        tx,
        s.gh.to_str().unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(result.status, ImplementationStatus::AwaitingApproval);
    assert_eq!(result.base_commit, remote);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        remote
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        fs::read_to_string(s.repo.join("local-only.txt")).unwrap(),
        "local work"
    );
    assert!(!result.task_repository.join("local-only.txt").exists());
}

#[test]
fn auto_mode_includes_remote_changes_that_arrive_during_implementation() {
    struct Advancing<'a> {
        sandbox: &'a Sandbox,
        calls: Arc<AtomicUsize>,
    }
    impl AiHarness for Advancing<'_> {
        fn label(&self) -> String {
            "advancing fixture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("fixture".into())
        }
        fn execute(
            &self,
            req: &PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, AppError> {
            self.sandbox.advance_remote();
            fs::write(
                self.sandbox.repo.join("local-only.txt"),
                "preserved local work",
            )
            .unwrap();
            self.sandbox
                .git(&self.sandbox.repo, &["add", "local-only.txt"]);
            self.sandbox.git(
                &self.sandbox.repo,
                &["commit", "-qm", "local work during implementation"],
            );
            Fixture {
                mode: "complete",
                calls: self.calls.clone(),
            }
            .execute(req)
        }
    }
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let (tx, _rx) = mpsc::channel();
    let result = run_with_options(
        &s.repo,
        &s.ticket,
        &Advancing {
            sandbox: &s,
            calls: calls.clone(),
        },
        Arc::new(AtomicBool::new(false)),
        tx,
        s.gh.to_str().unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(result.status, ImplementationStatus::AwaitingApproval);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(s.repo.join("local-only.txt")).unwrap(),
        "preserved local work"
    );
    assert!(
        s.git(&s.repo, &["log", "-1", "--format=%s"])
            .contains("local work during implementation")
    );
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["show", "main:upstream.txt"]),
        "latest upstream"
    );
    assert!(
        !s.git(
            &s.root.join("remote.git"),
            &["ls-tree", "-r", "--name-only", "main"]
        )
        .contains("implemented.txt")
    );
}
