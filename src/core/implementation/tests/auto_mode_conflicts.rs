use super::*;

#[test]
fn auto_mode_repairs_conflicts_and_keeps_verified_integration_for_review() {
    struct Conflicting<'a> {
        sandbox: &'a Sandbox,
        calls: Arc<AtomicUsize>,
    }
    impl AiHarness for Conflicting<'_> {
        fn label(&self) -> String {
            "conflicting fixture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("fixture".into())
        }
        fn execute(
            &self,
            req: &PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, AppError> {
            if self.calls.load(Ordering::SeqCst) == 0 {
                self.sandbox.advance_remote();
                let peer = self.sandbox.root.join("peer");
                fs::write(
                    peer.join("implemented.txt"),
                    "concurrent upstream implementation",
                )
                .unwrap();
                self.sandbox.git(&peer, &["add", "."]);
                self.sandbox
                    .git(&peer, &["commit", "-qm", "concurrent change"]);
                self.sandbox.git(&peer, &["push", "-q", "origin", "main"]);
            } else {
                assert!(req.prompt_body.contains("Integration verification failure"));
                assert!(req.prompt_body.contains("merge conflicts"));
            }
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
        &Conflicting {
            sandbox: &s,
            calls: calls.clone(),
        },
        Arc::new(AtomicBool::new(false)),
        tx,
        s.gh.to_str().unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(result.status, ImplementationStatus::ReadyToPublish);
    assert!(
        result
            .independent_check
            .as_ref()
            .is_some_and(|check| check.status == IndependentCheckStatus::Passed)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let remote = s.root.join("remote.git");
    assert_eq!(s.git(&remote, &["rev-list", "--count", "main"]), "3");
    assert_eq!(
        s.git(&remote, &["show", "main:implemented.txt"]),
        "concurrent upstream implementation"
    );
    assert_eq!(
        s.git(&remote, &["show", "main:upstream.txt"]),
        "latest upstream"
    );
}

#[test]
fn failed_auto_verification_never_changes_main() {
    let s = Sandbox::new();
    let initial = s.git(&s.root.join("remote.git"), &["rev-parse", "main"]);
    let (tx, _rx) = mpsc::channel();
    assert!(
        run_with_options(
            &s.repo,
            &s.ticket,
            &Fixture {
                mode: "fail",
                calls: Arc::new(AtomicUsize::new(0))
            },
            Arc::new(AtomicBool::new(false)),
            tx,
            "must-not-run",
            true
        )
        .is_err()
    );
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        initial
    );
}

#[test]
fn incomplete_acceptance_evidence_fails_closed() {
    let report = Report {
        status: ReportStatus::Complete,
        blocker_disposition: BlockerDisposition::None,
        summary: "Done".into(),
        acceptance_criteria: vec![report::Criterion {
            criterion: "One".into(),
            evidence: "Proof".into(),
        }],
        verification: vec!["cargo test".into()],
        remaining: vec![],
        human_choices: vec![],
    };
    assert!(validate_report(&report, "## Acceptance criteria\n- One\n- Two\n").is_err());
}
