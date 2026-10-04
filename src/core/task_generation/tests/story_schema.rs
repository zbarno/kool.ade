use super::*;

#[test]
fn outline_schema_errors_explain_reference_array_types_and_bare_json() {
    let response = r#"{"schema_version":1,"task_stories":null,"task_outline":[{"title":"Add the cleanup result","purpose":"Keep cleanup evidence after a safe cleanup pass.","target_repository":"root","scope_items":[1],"success_criteria":"Keep the marker after restart","dependencies":[]}]}"#;
    let error = decode_outline(response).unwrap_err().join(" ");
    assert!(error.contains("success_criteria must be arrays of one-based integer indexes"));
    assert!(error.contains("Return one bare JSON object"));
    assert!(SYSTEM.contains("bare JSON object"));
    assert!(SYSTEM.contains("do not add a preface"));
    assert!(TASK_OUTLINE_STEP.contains("Never put explanatory strings or prose"));
    assert!(TASK_OUTLINE_STEP.contains("success_criteria: array of one-based integer indexes"));
    assert!(TASK_OUTLINE_STEP.contains("Split distinct behaviors and safety contracts"));
    assert!(TASK_OUTLINE_STEP.contains("No one task should own the complete"));
    assert!(TASK_OUTLINE_STEP.contains("This step has no repository tools"));
}

#[test]
fn outline_decoder_accepts_one_complete_bare_object_after_a_preface() {
    let fixture = include_str!("../../../../tests/fixtures/task-outline.json");
    let response =
        format!("I have enough context to outline the work; indexes are {{1, 2, 4}}.\n{fixture}");
    assert_eq!(decode_outline(&response).unwrap().len(), 2);
}

#[test]
fn scope_changes_are_rejected_but_wording_and_reference_order_are_stable() {
    let outlines =
        decode_outline(include_str!("../../../../tests/fixtures/task-outline.json")).unwrap();
    let planned = &outlines[0];
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/task-story-1.json")).unwrap();
    value["task_stories"][0]["title"] = "Different wording".into();
    value["task_stories"][0]["purpose"] = "Paraphrased purpose".into();
    let story = story_response(&value.to_string(), planned, 0).unwrap();
    assert_eq!(story.title, planned.title);
    assert_eq!(story.purpose, planned.purpose);
    value["task_stories"][0]["scope_items"] = serde_json::json!([999]);
    assert!(
        story_response(&value.to_string(), planned, 0).unwrap_err()[0].contains("approved mapping")
    );
    assert!(same_refs(&[1, 2], &[2, 1]));
}
#[test]
fn missing_detail_produces_specific_repair_feedback() {
    let outlines =
        decode_outline(include_str!("../../../../tests/fixtures/task-outline.json")).unwrap();
    let errors = story_response(r#"{"task_stories":[{}]}"#, &outlines[0], 0)
        .unwrap_err()
        .join(" ");
    for field in [
        "intent",
        "goal",
        "context",
        "affected files",
        "implementation steps",
        "acceptance",
        "test plan",
        "verification",
        "definition of done",
    ] {
        assert!(
            errors.contains(field),
            "missing repair guidance for {field}: {errors}"
        );
    }
    assert!(!errors.contains("edge cases"));
    assert!(decode_stories(r#"{"task_stories":["#).is_err());
}

#[test]
fn malformed_story_json_gets_syntax_specific_repair_feedback() {
    let error = decode_stories(r#"{"task_stories":[{"title":"Broken"}]]}"#)
        .unwrap_err()
        .join(" ");
    assert!(error.contains("Invalid task story JSON"));
    assert!(error.contains("malformed"));
    assert!(error.contains("under 3,000 characters"));
    assert!(error.contains("exactly one task_stories entry"));
    assert!(error.contains("one flat array of strings"));
}

#[test]
fn malformed_story_repair_discards_the_broken_long_response() {
    let feedback = vec!["Invalid task story JSON: expected a comma".into()];
    let prior = previous_response_for_repair(&feedback, &"broken draft ".repeat(10_000));
    assert!(prior.contains("Discard it completely"));
    assert!(prior.contains("single-level arrays of strings"));
    assert!(!prior.contains("broken draft"));
    assert!(repair_is_response_only(&feedback));
}

#[test]
fn incomplete_story_json_explains_that_apostrophes_are_not_escaped() {
    let error = decode_stories("The repository's current behavior is...")
        .unwrap_err()
        .join(" ");
    assert!(error.contains("Apostrophes are ordinary characters"));
    assert!(error.contains("under 3,000 characters"));
}

#[test]
fn story_schema_repair_requires_response_envelope_and_budget() {
    let error = decode_stories(r#"{"acceptance_criteria":["visible"]}"#)
        .unwrap_err()
        .join(" ");
    assert!(error.contains("root must be the response envelope"));
    assert!(error.contains("inside the single task_stories array entry"));
    assert!(error.contains("under 4,000 characters"));
}

#[test]
fn nested_story_lists_get_element_type_guidance() {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/task-story-1.json")).unwrap();
    value["task_stories"][0]["edge_cases"] = serde_json::json!([["nested"]]);
    let error = decode_stories(&value.to_string()).unwrap_err().join(" ");
    assert!(error.contains("list field contains a nested array"));
    assert!(error.contains("arrays of strings"));
}
