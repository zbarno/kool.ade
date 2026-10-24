use super::*;

#[path = "initial_reconciliation/auto_verify.rs"]
mod auto_verify;
#[path = "initial_reconciliation/cache_reuse.rs"]
mod cache_reuse;
#[path = "initial_reconciliation/conflicts.rs"]
mod conflicts;
#[path = "initial_reconciliation/crash_recovery.rs"]
mod crash_recovery;
#[path = "initial_reconciliation/legacy_recovery.rs"]
mod legacy_recovery;
#[path = "initial_reconciliation/merge_recovery.rs"]
mod merge_recovery;
#[path = "initial_reconciliation/publication_retry.rs"]
mod publication_retry;
#[path = "initial_reconciliation/report_envelope.rs"]
mod report_envelope;
#[path = "initial_reconciliation/resilience.rs"]
mod resilience;
#[path = "initial_reconciliation/runtime_config.rs"]
mod runtime_config;
#[path = "initial_reconciliation/support.rs"]
mod support;
#[path = "initial_reconciliation/whitespace.rs"]
mod whitespace;
pub(super) use support::{complete_report, outcome, save_legacy_reconciliation_state};

struct ReviewerAwareAgent<'a>(&'a dyn AiHarness);

impl AiHarness for ReviewerAwareAgent<'_> {
    fn label(&self) -> String {
        self.0.label()
    }

    fn check_available(&self) -> Result<String, AppError> {
        self.0.check_available()
    }

    fn current_execution_identity(&self) -> Option<crate::harness::ReviewExecutionIdentity> {
        Some(crate::harness::ReviewExecutionIdentity {
            execution_id: "00000000-0000-4000-8000-000000000001".into(),
            session_id: "00000000-0000-4000-8000-000000000002".into(),
            runtime_qa_screenshot_review: None,
        })
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        if request.mode == crate::harness::ExecutionMode::ReadOnlyAnalysis {
            return Fixture {
                mode: "complete",
                calls: Arc::new(AtomicUsize::new(0)),
            }
            .execute(request);
        }
        self.0.execute(request)
    }
}

struct ReconcilingAgent {
    calls: Arc<AtomicUsize>,
    conflict: bool,
    block: bool,
    expect_snapshot: bool,
    wrap_report: bool,
}

impl AiHarness for ReconcilingAgent {
    fn label(&self) -> String {
        "reconciliation fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        if request.mode == crate::harness::ExecutionMode::ReadOnlyAnalysis {
            return Fixture {
                mode: "complete",
                calls: self.calls.clone(),
            }
            .execute(request);
        }
        assert_eq!(request.mode, crate::harness::ExecutionMode::Implementation);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if request.prompt_body.contains("INITIAL BASE RECONCILIATION") {
            assert!(request.prompt_body.contains("LOCAL COMMIT:"));
            assert!(request.prompt_body.contains("SHARED COMMIT:"));
            assert!(request.repo_root.join("upstream.txt").exists());
            if self.expect_snapshot {
                assert!(!request.repo_root.join("remote-later.txt").exists());
                assert!(!request.repo_root.join("local-later.txt").exists());
            }
            if self.conflict {
                assert!(request.prompt_body.contains("shared.txt"));
                if !self.block {
                    fs::write(
                        request.repo_root.join("shared.txt"),
                        "both edits preserved\n",
                    )
                    .unwrap();
                }
            } else {
                assert!(request.repo_root.join("local.txt").exists());
            }
            if self.block {
                return Ok(outcome(serde_json::json!({
                    "schemaVersion":2,
                    "status":"blocked",
                    "blocker_disposition":"human_action",
                    "summary":"The local and shared edits require an operator decision.",
                    "acceptance_criteria":[],
                    "verification":[],
                    "remaining":["Operator: choose how shared.txt should behave."],
                    "human_choices":[
                        {"id":"keep-local","label":"Keep local behavior","meaning":"Use the local shared.txt implementation.","consequence":"The remote behavior is not included."},
                        {"id":"keep-remote","label":"Keep shared behavior","meaning":"Use the fetched shared implementation.","consequence":"The local behavior is not included."}
                    ]
                })));
            }
            let command = if self.conflict {
                "test \"$(cat shared.txt)\" = 'both edits preserved'"
            } else {
                "test -f local.txt && test -f upstream.txt"
            };
            let mut response = outcome(complete_report(command));
            if self.wrap_report {
                response.final_text = format!(
                    "Reconciliation report follows.\n\n```json\n{}\n```",
                    response.final_text
                );
            }
            return Ok(response);
        }

        fs::write(request.repo_root.join("implemented.txt"), "implemented\n").unwrap();
        Ok(outcome(serde_json::json!({
            "schemaVersion":2,
            "status":"complete",
            "blocker_disposition":"none",
            "summary":"Implemented the requested task on the reconciled base.",
            "acceptance_criteria":[{"criterion":"File contains implemented.","evidence":"implemented.txt contains the expected content."}],
            "verification":["test \"$(cat implemented.txt)\" = implemented"],
            "remaining":[],
            "human_choices":[]
        })))
    }
}

fn conflict_sandbox() -> Sandbox {
    let s = Sandbox::new();
    fs::write(s.repo.join("shared.txt"), "common\n").unwrap();
    s.git(&s.repo, &["add", "shared.txt"]);
    s.git(&s.repo, &["commit", "-qm", "add shared file"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    s.advance_remote();
    let peer = s.latest_peer();
    fs::write(peer.join("shared.txt"), "shared implementation\n").unwrap();
    s.git(&peer, &["add", "shared.txt"]);
    s.git(&peer, &["commit", "-qm", "shared implementation"]);
    s.git(&peer, &["push", "-q", "origin", "main"]);
    fs::write(s.repo.join("shared.txt"), "local implementation\n").unwrap();
    s.git(&s.repo, &["add", "shared.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local implementation"]);
    s
}

fn run_with_agent(
    sandbox: &Sandbox,
    agent: &dyn AiHarness,
    user_context: Option<&str>,
) -> anyhow::Result<Implementation> {
    let (progress, _rx) = mpsc::channel();
    run_with_agent_for_ticket(sandbox, &sandbox.ticket, agent, user_context, progress)
}

fn run_with_agent_for_ticket(
    sandbox: &Sandbox,
    ticket: &str,
    agent: &dyn AiHarness,
    user_context: Option<&str>,
    progress: Sender<LiveProgress>,
) -> anyhow::Result<Implementation> {
    super::sandbox::refresh_task_spec(&sandbox.repo, ticket);
    run_with_project_options(
        &sandbox.repo,
        &sandbox.repo,
        ticket,
        RunOptions {
            harness: &ReviewerAwareAgent(agent),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode: PublicationMode::CreatePullRequest,
            require_independent_checks: false,
            user_context,
            auto_publish_gate: None,
            claim_lease: None,
        },
    )
}
