use super::*;

#[test]
fn main_turn_receives_the_model_selected_authoritative_context() {
    let (inputs, dir) = inputs_for("retrieval_pipeline", "The session vanishes after restart.");
    let product = dir.join(".kool-ade-packet/planning/product/architecture-and-constraints.md");
    std::fs::write(
        &product,
        "# Architecture and Constraints\n\nSESSION_CONTEXT_FROM_RETRIEVAL\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/session.rs"),
        "// session restore implementation\n",
    )
    .unwrap();
    let harness = RetrievalPipelineHarness;
    match drain(&TurnController::start(inputs, Box::new(harness))) {
        TurnOutcome::Applied { .. } => {}
        other => panic!("retrieval-backed planning turn should apply: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn turn_refuses_to_overwrite_rival_checkpoint_landed_during_the_run() {
    let (inputs, dir) = inputs_for("midflight_rival", "Note: adopt corporate SSO everywhere.");
    let spec_path = dir
        .join(crate::artifacts::product_docs::PRODUCT_DIR)
        .join("overview.md");
    let heads_before = head_count(&dir);
    let raw = serde_json::json!({"schema_version":1, "assistant_message":"Adopted corporate SSO.",
            "change_summary":"adopt sso",
            "document_updates":vision_update("Authentication uses corporate SSO.")})
    .to_string();
    let c = TurnController::start(
        inputs,
        Box::new(RivalCheckpoint {
            raw,
            root: dir.clone(),
            path: spec_path.clone(),
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
        _ => panic!("expected the stale snapshot to be refused"),
    }
    // The rival's checkpoint stands: neither reverted nor clobbered,
    // and the refused turn added no commit of its own.
    let text = std::fs::read_to_string(&spec_path).unwrap();
    assert!(text.contains("<!-- rival writer -->"));
    assert_eq!(
        head_count(&dir),
        heads_before + 4,
        "Every bounded retry observes a new rival commit; none may be overwritten"
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
                None,
                "Which provider?".into(),
                "Access".into(),
            )
        })
        .collect();
    std::fs::write(
        root.join(crate::artifacts::OPEN_ITEMS_FILE),
        crate::artifacts::items_io::serialize(&inputs.state.items),
    )
    .unwrap();
    inputs.state = PlannerState::load(&root).unwrap();
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
    let state = PlannerState::load(&root).unwrap();
    for item in &state.items {
        assert_eq!(item.evidence, format!("Answer {}", item.id));
    }
    let _ = std::fs::remove_dir_all(root);
}
