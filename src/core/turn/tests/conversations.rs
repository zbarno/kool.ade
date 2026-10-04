use super::*;

#[test]
fn quiescence_longer_than_one_poll_interval_is_not_a_vanished_turn() {
    // Regression: `drain` used to declare the turn vanished on the first
    // 250ms idle poll, but a loaded machine lets the worker sit quiet
    // longer than one poll interval before it emits anything. Quiescence
    // is not death; only the worker handle proving the turn gone is.
    struct SilentUntilLate(ScriptedHarness);
    impl AiHarness for SilentUntilLate {
        fn label(&self) -> String {
            "silent-until-late".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("test".into())
        }
        fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            // Stay silent across two or more 250ms poll intervals.
            std::thread::sleep(Duration::from_millis(700));
            self.0.execute(req)
        }
    }
    let (inputs, _) = inputs_for("silent_quiet", "Quietly note the decision.");
    let env: TurnEnvelope = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "assistant_message": "Quietly noted.",
        "change_summary": "note decision",
    }))
    .unwrap();
    let c = TurnController::start(
        inputs,
        Box::new(SilentUntilLate(ScriptedHarness {
            canned: Some(env),
            raw: None,
        })),
    );
    match drain(&c) {
        TurnOutcome::Applied { .. } => {}
        other => panic!("expected an applied turn, got {other:?}"),
    }
}

#[test]
fn synthetic_ownership_conversation_keeps_identity_after_numbering_and_reload() {
    let (mut inputs, dir) = inputs_for("synthetic_conversation", "Who can assign this?");
    inputs.state.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "Security".into(),
        None,
        "Audit policy?".into(),
        "Controls access".into(),
    ));
    let gaps = crate::core::ownership::synthesize_missing_owners(
        &inputs.state.items,
        &inputs.state.config.stakeholders,
    );
    let key = gaps[0].conversation_key().to_string();
    let body =
        crate::core::task_conversation::prompt(&inputs.state, &key, &inputs.user_message, &[])
            .unwrap();
    assert!(body.contains("has no assigned stakeholder"));
    let c = TurnController::start_scoped(inputs, Box::new(ScriptedHarness {
            canned: None,
            raw: Some(serde_json::json!({"schema_version":1, "assistant_message":"Use Assign ownership to choose the responsible group.",
                "open_items_added":[], "open_items_updated":[], "open_items_resolved":[]}).to_string()),
        }), Some(key.clone()));
    assert!(matches!(drain(&c), TurnOutcome::Applied { .. }));
    let state = PlannerState::load(&dir).unwrap();
    let gap = state
        .items
        .iter()
        .find(|item| item.is_ownership_gap())
        .unwrap();
    assert!(gap.id.starts_with("CLR-"));
    assert_eq!(gap.conversation_key(), key);
    assert!(crate::core::task_conversation::prompt(&state, &key, "Continue", &[]).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn focused_conversation_persists_resolution_without_other_chat_context() {
    let (mut inputs, dir) = inputs_for("task_conversation", "Use corporate SSO");
    let item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "Which authentication provider?".into(),
        "Controls access".into(),
    );
    inputs.state.items.push(item);
    inputs.recent_chat = vec![("User".into(), "Our employees need access".into())];
    let body = crate::core::task_conversation::prompt(
        &inputs.state,
        "CLR-001",
        &inputs.user_message,
        &inputs.recent_chat,
    )
    .unwrap();
    assert!(body.contains("Which authentication provider?"));
    assert!(body.contains("Our employees need access"));
    assert!(
        crate::core::prompt::TASK_CONVERSATION_MODE_NOTE
            .contains("open_items_resolved is an array of item-ID strings")
    );
    assert!(!body.contains("=== INTERVIEW BRIEF ==="));
    let c = TurnController::start_scoped(inputs, Box::new(ScriptedHarness {
            canned: None,
            raw: Some(serde_json::json!({"schema_version":1, "assistant_message":"Recorded corporate SSO.",
                "change_summary":"record authentication provider", "document_updates":vision_update("Authentication uses corporate SSO."),
                "open_items_resolved":["CLR-001"]}).to_string()),
        }), Some("CLR-001".into()));
    match drain(&c) {
        TurnOutcome::Applied { .. } => {}
        other => panic!("expected applied task reply: {other:?}"),
    }
    let loaded = PlannerState::load(&dir).unwrap();
    assert!(!loaded.items.iter().any(|i| i.id == "CLR-001"));
    assert!(
        loaded
            .spec_text
            .unwrap()
            .contains("Authentication uses corporate SSO.")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn task_conversation_cannot_dispatch_a_project_application_action() {
    let (mut inputs, dir) = inputs_for("task_action_gate", "Start the task discussed here");
    inputs.state.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "Which export format should the task use?".into(),
        "The task needs a user decision.".into(),
    ));
    git_stdout(&dir, &["add", ".koolade-packet"]);
    git_stdout(&dir, &["commit", "-m", "Seed planning artifacts"]);
    let before = law_evidence(&dir);
    let response = serde_json::json!({
        "schema_version": 1,
        "assistant_message": "Starting implementation now.",
        "requested_action": {"action":"start_implementation"}
    })
    .to_string();
    let controller = TurnController::start_scoped(
        inputs,
        Box::new(ScriptedHarness {
            canned: None,
            raw: Some(response),
        }),
        Some("CLR-001".into()),
    );
    match drain(&controller) {
        TurnOutcome::Rejected { problems, .. } => assert!(
            problems
                .iter()
                .any(|problem| problem.contains("cannot run project-level actions"))
        ),
        other => panic!("task-level action must be rejected, got {other:?}"),
    }
    assert_eq!(law_evidence(&dir), before);
    let _ = std::fs::remove_dir_all(dir);
}
