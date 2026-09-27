use super::*;

#[test]
fn planning_turn_rejects_task_generation_fields_even_when_empty() {
    let error = decode_turn(
        r#"{"schema_version":2,"assistant_message":"Done.","task_stories":null}"#,
        TurnPurpose::Interview,
    )
    .unwrap_err();
    assert!(error.contains("unknown field"));
}

#[test]
fn task_generation_rejects_planning_mutations_before_normalization() {
    let error = decode_turn(
        r#"{"schema_version":1,"assistant_message":"Done.","task_stories":[],"document_updates":[]}"#,
        TurnPurpose::GenerateTasks,
    )
    .unwrap_err();
    assert!(error.contains("unknown field"));
}

#[test]
fn task_generation_normalizes_only_its_validated_story_payload() {
    let response = decode_turn(
        r#"{"schema_version":1,"assistant_message":"Stories are ready.","task_stories":[]}"#,
        TurnPurpose::GenerateTasks,
    )
    .unwrap();
    assert_eq!(response.schema_version, Some(2));
    assert_eq!(response.assistant(), "Stories are ready.");
    assert!(response.document_updates.is_none());
    assert!(response.task_stories.as_ref().is_some_and(Vec::is_empty));
}

#[test]
fn planning_legacy_version_is_normalized_at_the_decoder_boundary() {
    let response = decode_turn(
        r#"{"schema_version":1,"assistant_message":"Updated the old specification.","updated_specification":"legacy replacement text"}"#,
        TurnPurpose::Interview,
    )
    .unwrap();
    assert_eq!(response.schema_version, Some(2));
    assert!(response.updated_specification.is_some());
}

#[test]
fn planning_envelope_round_trips_two_plan_alternatives_and_advisory_recommendation() {
    let wire = r#"{
        "schema_version": 2,
        "assistant_message": "Compare the two approaches.",
        "plans": [
            {"id":"A","objective":"Ship safely","phases":["shadow","cut over"],"files_touched":["src/a.rs"],"state_changes":["Add marker"],"failure_modes":["Stale marker"],"effort_band":"small","known_risks":["Extra read"],"reversibility":"Remove marker"},
            {"id":"B","objective":"Ship in one step","phases":["replace"],"files_touched":["src/b.rs"],"state_changes":["Replace path"],"failure_modes":["Partial write"],"effort_band":"medium","known_risks":["Rollback"],"reversibility":"Restore backup"}
        ],
        "recommendation": {"plan_id":"A","rationale":"It has a safer transition.","evidence":["src/a.rs:12"]}
    }"#;
    let decoded = decode_turn(wire, TurnPurpose::Interview).unwrap();
    let plans = decoded.plans.as_ref().unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].phases, ["shadow", "cut over"]);
    assert_eq!(plans[1].known_risks, ["Rollback"]);
    assert_eq!(decoded.recommendation.as_ref().unwrap().plan_id, "A");

    let encoded = serde_json::to_string(&decoded).unwrap();
    let round_trip = decode_turn(&encoded, TurnPurpose::Interview).unwrap();
    assert_eq!(round_trip.plans, decoded.plans);
    assert_eq!(round_trip.recommendation, decoded.recommendation);
}

#[test]
fn planning_v1_without_comparison_fields_remains_backward_compatible() {
    let decoded = decode_turn(
        r#"{"schema_version":1,"assistant_message":"Legacy planning response."}"#,
        TurnPurpose::Interview,
    )
    .unwrap();
    assert_eq!(decoded.plans, None);
    assert_eq!(decoded.recommendation, None);
}

#[test]
fn task_generation_rejects_comparison_only_fields() {
    for field in ["plans", "recommendation"] {
        let value = serde_json::json!({"schema_version": 1, field: null});
        assert!(
            serde_json::from_value::<TaskGenerationResponse>(value).is_err(),
            "accepted comparison-only field {field}"
        );
    }
}

#[test]
fn unsupported_operation_versions_are_rejected_before_core_normalization() {
    let planning = decode_turn(
        r#"{"schema_version":3,"assistant_message":"Done."}"#,
        TurnPurpose::Interview,
    )
    .unwrap_err();
    assert!(planning.contains("Planning schema_version 3"));

    let task_generation = decode_turn(
        r#"{"schema_version":2,"assistant_message":"Done.","task_stories":[]}"#,
        TurnPurpose::GenerateTasks,
    )
    .unwrap_err();
    assert!(task_generation.contains("Task generation schema_version 2"));

    let investigation =
        decode_investigation(r#"{"schema_version":1,"assistant_message":"Done."}"#).unwrap_err();
    assert!(investigation.contains("Investigation schema_version 1"));
}

#[test]
fn reconciliation_rejects_planning_workflow_and_task_fields() {
    for field in ["interview", "task_outline", "requested_action"] {
        let value = serde_json::json!({
            "schema_version": 2,
            "assistant_message": "Reconciled.",
            field: null
        });
        assert!(
            serde_json::from_value::<ReconciliationResponse>(value).is_err(),
            "accepted {field}"
        );
    }
}

#[test]
fn task_outline_schema_rejects_specification_updates() {
    let value = serde_json::json!({
        "schema_version": 1,
        "task_outline": [],
        "updated_specification": "# unrelated"
    });
    assert!(serde_json::from_value::<TaskOutlineResponse>(value).is_err());
}

#[test]
fn decision_brief_schema_rejects_unrelated_transcript_fields() {
    let error = decode_decision_brief(
        r#"{"problem":"A specific issue.","after":"Resume after repair.","requested_action":{"action":"publish"}}"#,
    )
    .unwrap_err();
    assert!(error.contains("unknown field"));
}
