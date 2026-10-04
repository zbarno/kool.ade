use super::*;

struct EnvironmentBlockedAgent(Arc<AtomicUsize>);

impl AiHarness for EnvironmentBlockedAgent {
    fn label(&self) -> String {
        "environment blocker fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(&self, _: &PlanningRequest) -> Result<crate::harness::HarnessOutcome, AppError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(outcome(serde_json::json!({
            "schemaVersion":2,
            "status":"blocked",
            "blocker_disposition":"environment_prerequisite",
            "summary":"The locked frontend dependencies are not present in the sandbox cache.",
            "acceptance_criteria":[],
            "verification":["npm ci"],
            "remaining":["Seed the npm cache and rerun the frontend checks."],
            "human_choices":[]
        })))
    }
}

#[test]
fn preface_and_json_fence_are_removed_before_report_validation() {
    let sandbox = conflict_sandbox();
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: true,
        block: false,
        expect_snapshot: false,
        wrap_report: true,
    };

    let result = run_with_agent(&sandbox, &agent, None).unwrap();

    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    assert!(
        sandbox
            .git(
                &result.worktree,
                &["merge-base", "--is-ancestor", &result.base_commit, "HEAD"]
            )
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(result.worktree.join("shared.txt")).unwrap(),
        "both edits preserved\n"
    );
    assert!(result.worktree.join("upstream.txt").exists());
}

#[test]
fn environment_blocker_stops_without_consuming_report_retries() {
    let sandbox = conflict_sandbox();
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = EnvironmentBlockedAgent(calls.clone());

    let error = run_with_agent(&sandbox, &agent, None).unwrap_err();
    let failure = Failure::from_error(&error);

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(failure.kind, FailureKind::ExternalPrerequisite);
    assert_eq!(failure.recovery, RecoveryDisposition::UserAction);
    assert!(failure.message.contains("Waiting for environment"));
    assert!(failure.message.contains("Seed the npm cache"));
    let saved = load(&sandbox.repo, &sandbox.ticket).unwrap();
    assert!(saved.worktree.is_dir());
    assert!(
        !sandbox
            .git(&saved.worktree, &["rev-parse", "--verify", "MERGE_HEAD"])
            .is_empty()
    );
}
