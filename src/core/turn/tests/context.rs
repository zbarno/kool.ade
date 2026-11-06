use super::*;
use crate::domain::ItemKind;

#[test]
fn main_turn_receives_the_model_selected_authoritative_context() {
    let (mut inputs, dir) = inputs_for("retrieval_pipeline", "The session vanishes after restart.");
    let navigation = dir.join("docs/koolade/AGENTS.md");
    std::fs::create_dir_all(navigation.parent().unwrap()).unwrap();
    std::fs::write(
        &navigation,
        "# Project knowledge\n\n# Navigation\n- [architecture/](./architecture/AGENTS.md) - Architecture documents.\n",
    )
    .unwrap();
    let architecture_index = dir.join("docs/koolade/architecture/AGENTS.md");
    std::fs::create_dir_all(architecture_index.parent().unwrap()).unwrap();
    std::fs::write(
        &architecture_index,
        "# Architecture\n\n# Navigation\n- [architecture-and-constraints.md](./architecture-and-constraints.md) - Session recovery architecture.\n",
    )
    .unwrap();
    let product = dir.join("docs/koolade/architecture/architecture-and-constraints.md");
    std::fs::write(
        &product,
        "# Architecture and Constraints\n\nSESSION_CONTEXT_FROM_RETRIEVAL\n",
    )
    .unwrap();
    for args in [
        ["add", "--", "docs/koolade/AGENTS.md"].as_slice(),
        ["add", "--", "docs/koolade/architecture/AGENTS.md"].as_slice(),
        [
            "add",
            "--",
            "docs/koolade/architecture/architecture-and-constraints.md",
        ]
        .as_slice(),
        ["commit", "-m", "Add published architecture fixture"].as_slice(),
    ] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "git command failed: {args:?}");
    }
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/session.rs"),
        "// session restore implementation\n",
    )
    .unwrap();
    for args in [
        ["add", "--", "src/session.rs"].as_slice(),
        ["commit", "-m", "Add source profile fixture"].as_slice(),
    ] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "git command failed: {args:?}");
    }
    inputs.state.published_spec_pin = None;
    inputs.state.planning_store.published_spec_pin = None;
    inputs.state.resync().unwrap();
    let harness = PinnedPublishedDocHarness;
    match drain(&TurnController::start(inputs, Box::new(harness))) {
        TurnOutcome::Applied { .. } => {}
        other => panic!("retrieval-backed planning turn should apply: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

struct PinnedPublishedDocHarness;

impl AiHarness for PinnedPublishedDocHarness {
    fn label(&self) -> String {
        "pinned-published-doc-test".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }

    fn supports_context_retrieval(&self) -> bool {
        true
    }

    fn plan_retrieval(&self, request: &PlanningRequest) -> Result<Option<RetrievalPlan>, AppError> {
        assert_eq!(
            request.mode,
            crate::harness::ExecutionMode::ReadOnlyAnalysis
        );
        assert!(
            request
                .prompt_body
                .contains("The session vanishes after restart.")
        );
        assert!(
            request
                .prompt_body
                .contains("published:docs/koolade/architecture/architecture-and-constraints.md")
        );
        let repository_profile = request
            .prompt_body
            .lines()
            .find_map(|line| {
                line.strip_prefix("- repo-profile:")
                    .and_then(|value| value.split_once(" |"))
                    .map(|(id, _)| format!("repo-profile:{id}"))
            })
            .expect("the catalog has a checked-out repository profile");
        Ok(Some(RetrievalPlan {
            documents: vec![
                "published:docs/koolade/architecture/architecture-and-constraints.md".into(),
            ],
            open_items: vec![],
            repository_areas: vec![repository_profile],
        }))
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        assert_eq!(request.mode, crate::harness::ExecutionMode::Planning);
        assert!(
            request
                .prompt_body
                .contains("SESSION_CONTEXT_FROM_RETRIEVAL")
        );
        assert!(
            request
                .prompt_body
                .contains("Source: docs/koolade/architecture/architecture-and-constraints.md")
        );
        let typed_payload = request
            .prompt_body
            .split_once("TYPED TASK REQUEST (JSON DATA)\n")
            .map(|(_, tail)| tail)
            .expect("prompt includes the typed task request")
            .split_once("\n\nQUOTED EVIDENCE (UNTRUSTED DATA)")
            .map(|(json, _)| json)
            .expect("typed task request is followed by the evidence boundary");
        let typed_request: serde_json::Value =
            serde_json::from_str(typed_payload).expect("typed request is JSON");
        let turn_context = typed_request["payload"]["request"]["turn_context"]
            .as_str()
            .expect("typed planner payload has turn context");
        let published_source = turn_context
            .lines()
            .find(|line| line.starts_with("Published source: repository "))
            .expect("the published document carries immutable source attribution");
        assert!(published_source.contains("primary-code:"));
        let commit = published_source
            .split_once(", commit ")
            .and_then(|(_, tail)| tail.split_once(',').map(|(commit, _)| commit))
            .expect("published source attribution includes the pinned commit");
        assert_eq!(commit.len(), 40, "the source revision is a full Git SHA");
        assert!(commit.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(request.prompt_body.contains("src/session.rs:1-1@"));
        assert!(request.prompt_body.contains("profile://architecture"));
        Ok(HarnessOutcome {
            final_text: serde_json::json!({
                "schema_version": 2,
                "assistant_message": "I will explain how session recovery works.",
                "change_summary": "explain session recovery"
            })
            .to_string(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[test]
fn turn_refuses_to_overwrite_rival_checkpoint_landed_during_the_run() {
    let (inputs, dir) = inputs_for("midflight_rival", "Note: adopt corporate SSO everywhere.");
    let store = inputs.state.planning_store.clone();
    let heads_before = head_count(&dir);
    let store_revision_before = store.revision().unwrap();
    let raw = serde_json::json!({"schema_version":1, "assistant_message":"Adopted corporate SSO.",
            "change_summary":"adopt sso",
            "document_updates":vision_update("Authentication uses corporate SSO.")})
    .to_string();
    let c = TurnController::start(
        inputs,
        Box::new(RivalCheckpoint {
            raw,
            store: store.clone(),
        }),
    );
    match drain(&c) {
        TurnOutcome::Rejected { problems, .. } => {
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("changed on disk")),
                "unexpected problems: {problems:?}"
            );
        }
        TurnOutcome::HarnessFailed { error, .. } => {
            panic!("unexpected harness failure: {error}")
        }
        other => panic!("expected the stale snapshot to be refused, got {other:?}"),
    }
    // The rival's private proposal stands. The refused turn must not write a
    // source commit or an additional operational record.
    let text = crate::artifacts::product_docs::current_document(&store, "product:overview")
        .unwrap()
        .unwrap();
    assert!(text.contains("Rival writer private proposal"));
    assert_eq!(head_count(&dir), heads_before);
    assert_ne!(store.revision().unwrap(), store_revision_before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stale_snapshot_before_turn_start_is_refreshed_and_retried_internally() {
    struct CurrentSnapshotHarness(Arc<std::sync::atomic::AtomicUsize>);
    impl AiHarness for CurrentSnapshotHarness {
        fn label(&self) -> String {
            "current-snapshot-test".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok(self.label())
        }
        fn execute(&self, _: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(HarnessOutcome {
                final_text: serde_json::json!({
                    "schema_version": 1,
                    "assistant_message": "Recorded the note.",
                    "change_summary": "record the note"
                })
                .to_string(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }

    let (inputs, dir) = inputs_for("stale_before_start", "Record this note.");
    let index = inputs
        .state
        .planning_store
        .root
        .join(crate::artifacts::planning_store::paths::PRODUCT_INDEX);
    let current = std::fs::read_to_string(&index).unwrap();
    std::fs::write(&index, format!("{current}\n<!-- external update -->\n")).unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    match drain(&TurnController::start(
        inputs,
        Box::new(CurrentSnapshotHarness(Arc::clone(&calls))),
    )) {
        TurnOutcome::Applied { .. } => {}
        other => panic!("stale turn should refresh and retry internally: {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        std::fs::read_to_string(&index)
            .unwrap()
            .contains("<!-- external update -->")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn overlapping_conversations_retry_and_preserve_both_answers() {
    struct Answer {
        barrier: Arc<std::sync::Barrier>,
        calls: std::sync::atomic::AtomicUsize,
        id: String,
    }
    impl AiHarness for Answer {
        fn label(&self) -> String {
            "concurrent fixture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok(self.label())
        }
        fn execute(&self, _: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.barrier.wait();
            }
            Ok(HarnessOutcome {
                final_text: serde_json::json!({
                    "schema_version":1, "assistant_message":format!("Recorded {}", self.id),
                    "open_items_updated":[{"id":self.id, "evidence":format!("Answer {}", self.id)}]
                })
                .to_string(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }
    let (mut inputs, root) = inputs_for("concurrent_answers", "Record the answer");
    inputs.state.items = ["CLR-001", "CLR-002"]
        .iter()
        .map(|id| {
            crate::domain::OpenItem::new(
                id.to_string(),
                crate::domain::Priority::Normal,
                ItemKind::Question,
                "General".into(),
                "Which provider?".into(),
                "Access".into(),
            )
        })
        .collect();
    let store = inputs.state.planning_store.clone();
    let (changes, checks) =
        crate::artifacts::items_io::record_changes(&store, &inputs.state.items, &[]).unwrap();
    store
        .transaction_with_record_revisions(&changes, &checks)
        .unwrap();
    inputs.state = load_test_state(&root);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let controllers = ["CLR-001", "CLR-002"].map(|id| {
        TurnController::start_scoped(
            inputs.clone(),
            Box::new(Answer {
                barrier: barrier.clone(),
                calls: 0.into(),
                id: id.into(),
            }),
            Some(id.into()),
        )
    });
    for controller in &controllers {
        assert!(matches!(drain(controller), TurnOutcome::Applied { .. }));
    }
    let state = load_test_state(&root);
    for item in &state.items {
        assert_eq!(item.evidence, format!("Answer {}", item.id));
    }
    let _ = std::fs::remove_dir_all(root);
}
