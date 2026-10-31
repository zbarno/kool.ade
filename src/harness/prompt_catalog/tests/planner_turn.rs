use super::*;

fn planner_turn_input() -> RoleInputV1 {
    base(
        PromptRole::PlannerConversationV5,
        RolePayloadV1::PlannerTurn {
            turn_context: "Current user request and retrieved project context.".into(),
            workflow_instructions: Some("Interview workflow rules.".into()),
            operator_persona: "Be concise.".into(),
            task_mode_note: Some("TASK CONVERSATION MODE".into()),
        },
    )
}

#[test]
fn historical_planner_v4_manifest_remains_valid() {
    let input = base(
        PromptRole::PlannerConversation,
        RolePayloadV1::PlannerTurn {
            turn_context: "historical context".into(),
            workflow_instructions: None,
            operator_persona: "historical persona".into(),
            task_mode_note: None,
        },
    );
    let manifest = render(&input).unwrap().manifest;
    assert_eq!(manifest.role_version, "planner/v4");
    assert_eq!(
        validate_prompt_manifest(&manifest).unwrap(),
        PromptRole::PlannerConversation
    );
    let output = r#"{"schema_version":2,"assistant_message":"Historical response"}"#;
    assert!(matches!(
        validate_output_with_manifest(&manifest, output, &Default::default()).unwrap(),
        RoleOutputV1::PlanningTurn(_)
    ));
}

#[test]
fn planning_turn_catalog_keeps_dynamic_context_out_of_trusted_system() {
    let rendered = render(&planner_turn_input()).unwrap();
    assert_eq!(rendered.manifest.role_version, "planner_conversation/v5");
    assert_eq!(
        rendered.manifest.output_schema_version,
        "PlanningTurnResponseV1"
    );
    assert!(rendered.system.contains("Planning Analyst for Kool.ad/e"));
    assert!(
        rendered
            .system
            .contains("separate managed planning-repository root")
    );
    assert!(
        !rendered
            .system
            .contains(".koolade-packet/planning/product/")
    );
    assert!(
        rendered
            .system
            .contains("existing general capabilities or workarounds")
    );
    assert!(!rendered.system.contains("Be concise."));
    assert!(
        rendered
            .user
            .contains("Current user request and retrieved project context.")
    );
    assert!(rendered.user.contains("Interview workflow rules."));
    assert!(rendered.user.contains("Be concise."));
    assert!(rendered.user.contains("TASK CONVERSATION MODE"));
}

#[test]
fn planning_turn_output_uses_existing_operation_decoder_and_rejects_invalid_claims() {
    let manifest = render(&planner_turn_input()).unwrap().manifest;
    let output = r#"{"schema_version":2,"assistant_message":"Ready","requested_action":{"action":"publish"}}"#;
    let decoded = validate_output_with_manifest(&manifest, output, &Default::default()).unwrap();
    assert!(matches!(decoded, RoleOutputV1::PlanningTurn(_)));

    let malformed =
        r#"{"schema_version":2,"assistant_message":"Ready","unrecognized_authority":true}"#;
    assert!(validate_output_with_manifest(&manifest, malformed, &Default::default()).is_err());

    let purpose = crate::core::workflow::TurnPurpose::ComparePlans;
    let forbidden = r#"{"schema_version":2,"task_stories":[],"plans":[],"recommendation":null}"#;
    assert!(
        validate_output_with_manifest(
            &manifest,
            forbidden,
            &OutputValidationContext {
                turn_purpose: purpose,
                ..Default::default()
            }
        )
        .is_err()
    );
}
