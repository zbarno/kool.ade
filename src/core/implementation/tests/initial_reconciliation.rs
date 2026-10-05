use super::*;

#[path = "initial_reconciliation/auto_verify.rs"]
mod auto_verify;
#[path = "initial_reconciliation/cache_reuse.rs"]
mod cache_reuse;
#[path = "initial_reconciliation/crash_recovery.rs"]
mod crash_recovery;
#[path = "initial_reconciliation/legacy_recovery.rs"]
mod legacy_recovery;
#[path = "initial_reconciliation/merge_recovery.rs"]
mod merge_recovery;
#[path = "initial_reconciliation/report_envelope.rs"]
mod report_envelope;
#[path = "initial_reconciliation/resilience.rs"]
mod resilience;
#[path = "initial_reconciliation/runtime_config.rs"]
mod runtime_config;
#[path = "initial_reconciliation/whitespace.rs"]
mod whitespace;

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

#[test]
fn divergent_histories_are_combined_before_the_task_agent_runs() {
    let s = Sandbox::new();
    let remote = s.advance_remote();
    fs::write(s.repo.join("local.txt"), "local change\n").unwrap();
    s.git(&s.repo, &["add", "local.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    assert!(
        s.git(
            &result.worktree,
            &["merge-base", "--is-ancestor", &local, &result.base_commit]
        )
        .is_empty()
    );
    assert!(
        s.git(
            &result.worktree,
            &["merge-base", "--is-ancestor", &remote, &result.base_commit]
        )
        .is_empty()
    );
    assert_eq!(
        fs::read_to_string(result.worktree.join("local.txt")).unwrap(),
        "local change\n"
    );
    assert_eq!(
        fs::read_to_string(result.worktree.join("upstream.txt")).unwrap(),
        "latest upstream\n"
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        remote
    );
}

#[test]
fn merge_conflicts_are_agent_resolved_and_verified_in_isolation() {
    let s = conflict_sandbox();
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let remote = s.git(&s.root.join("remote.git"), &["rev-parse", "main"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: true,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        fs::read_to_string(result.worktree.join("shared.txt")).unwrap(),
        "both edits preserved\n"
    );
    assert!(
        s.git(
            &result.worktree,
            &["merge-base", "--is-ancestor", &local, &result.base_commit]
        )
        .is_empty()
    );
    assert!(
        s.git(
            &result.worktree,
            &["merge-base", "--is-ancestor", &remote, &result.base_commit]
        )
        .is_empty()
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        remote
    );
}

#[test]
fn ambiguous_conflicts_remain_blocked_without_touching_either_branch() {
    let s = conflict_sandbox();
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: true,
        block: true,
        expect_snapshot: false,
        wrap_report: false,
    };

    let error = run_with_agent(&s, &agent, None).unwrap_err().to_string();

    assert!(error.contains("choose how shared.txt should behave"));
    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert!(state.detail.contains("Keep local behavior"));
    assert!(!state.worktree.join("implemented.txt").exists());
    assert!(
        !s.git(&state.worktree, &["diff", "--name-only", "--diff-filter=U"])
            .is_empty()
    );
}

fn conflict_sandbox() -> Sandbox {
    let s = Sandbox::new();
    fs::write(s.repo.join("shared.txt"), "common\n").unwrap();
    s.git(&s.repo, &["add", "shared.txt"]);
    s.git(&s.repo, &["commit", "-qm", "add shared file"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    s.advance_remote();
    let peer = s.root.join("peer");
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
    run_with_project_options(
        &sandbox.repo,
        &sandbox.repo,
        ticket,
        RunOptions {
            harness: agent,
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode: PublicationMode::CreatePullRequest,
            require_independent_checks: false,
            user_context,
            auto_publish_gate: None,
        },
    )
}

fn complete_report(command: &str) -> serde_json::Value {
    let criteria = crate::core::implementation::initial_reconciliation::CONTRACT
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|criterion| serde_json::json!({"criterion":criterion,"evidence":"Both histories were checked and the combined baseline verification passed."}))
        .collect::<Vec<_>>();
    serde_json::json!({
        "schemaVersion":2,
        "status":"complete",
        "blocker_disposition":"none",
        "summary":"Both starting histories are integrated and the combined baseline passed its checks.",
        "acceptance_criteria":criteria,
        "verification":[command],
        "remaining":[],
        "human_choices":[]
    })
}

fn outcome(value: serde_json::Value) -> crate::harness::HarnessOutcome {
    crate::harness::HarnessOutcome {
        final_text: value.to_string(),
        envelope: None,
        stderr_tail: String::new(),
    }
}
