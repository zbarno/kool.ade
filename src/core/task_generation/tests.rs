use crate::core::task_generation::generation::model_batch_context;
use crate::core::task_generation::run::{
    previous_response_for_repair, repair_execution_mode, repair_is_response_only,
};
use crate::core::task_generation::{
    prompt::{STORY_CONTRACT, SYSTEM, TASK_OUTLINE_STEP},
    response::{decode_outline, decode_stories, same_refs, story_response},
    run::working_tree_fingerprint,
};

#[test]
fn detail_guidance_scales_to_the_issue_instead_of_word_counts() {
    assert!(STORY_CONTRACT.contains("Do not follow fixed word counts"));
    assert!(STORY_CONTRACT.contains("optional arrays empty"));
    assert!(STORY_CONTRACT.contains("exactly one object"));
    assert!(STORY_CONTRACT.contains("at most four relevant source"));
    assert!(STORY_CONTRACT.contains("avoid repeating the same requirement"));
    assert!(STORY_CONTRACT.contains("8,000 serialized characters"));
    assert!(STORY_CONTRACT.contains("4,000 characters"));
    assert!(STORY_CONTRACT.contains("steps 1,900"));
    assert!(STORY_CONTRACT.contains("definition\nof done 320"));
    assert!(STORY_CONTRACT.contains("Do not add behavior, interfaces, files, or guarantees"));
    assert!(!STORY_CONTRACT.contains("450+ words"));
    assert!(!STORY_CONTRACT.contains("700-1400 words"));
    assert!(!STORY_CONTRACT.contains("12+ words"));
}

#[test]
fn planning_work_status_does_not_invalidate_task_generation_checkpoints() {
    use std::{fs, process::Command};

    let root = std::env::temp_dir().join(format!(
        "packet-task-generation-fingerprint-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(root.join(".kool-ade-packet/state")).unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Packet Test"]);
    git(&["config", "user.email", "packet-test@example.invalid"]);
    fs::write(root.join("src.rs"), "base").unwrap();
    fs::write(root.join(".kool-ade-packet/state/work.json"), "[]").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);

    let clean = working_tree_fingerprint(&root).unwrap();
    fs::write(
        root.join(".kool-ade-packet/state/work.json"),
        "changed retry state",
    )
    .unwrap();
    assert_eq!(working_tree_fingerprint(&root), Some(clean));
    fs::write(root.join("src.rs"), "changed source").unwrap();
    assert_ne!(working_tree_fingerprint(&root), Some(clean));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn story_context_does_not_repeat_the_frozen_feature_specification() {
    let contract = serde_json::json!({
        "featureSpecification": "Full feature text already appears in the approved specification section.",
        "productModules": {"MOD-01": "Relevant module contract."},
        "repositoryBases": {"root": "abc123"},
        "configuration": "Not needed to author a task story."
    });

    let context = model_batch_context(&contract);

    assert!(context.get("feature_specification").is_none());
    assert!(context.get("configuration").is_none());
    assert_eq!(
        context["product_modules"]["MOD-01"],
        "Relevant module contract."
    );
    assert_eq!(context["repository_bases"]["root"], "abc123");
}

#[test]
fn oversized_story_is_rejected_with_compact_repair_guidance() {
    let outlines =
        decode_outline(include_str!("../../../tests/fixtures/task-outline.json")).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/task-story-1.json")).unwrap();
    value["task_stories"][0]["context"] = "grounded detail ".repeat(600).into();
    let errors = story_response(&value.to_string(), &outlines[0], 0).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("limit is 8000"));
    assert!(errors[0].contains("Remove at least"));
    assert!(errors[0].contains("target 4,000 characters or fewer"));
    assert!(errors[0].contains("Field budget overages"));
    assert!(errors[0].contains("Longest fields"));
    assert!(errors[0].contains("context"));
    assert!(errors[0].contains("test plan"));
}

#[test]
fn excessive_implementation_steps_get_a_targeted_budget_error() {
    let outlines =
        decode_outline(include_str!("../../../tests/fixtures/task-outline.json")).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/task-story-1.json")).unwrap();
    value["task_stories"][0]["implementation_steps"][0] = "specific change ".repeat(180).into();
    let error = story_response(&value.to_string(), &outlines[0], 0)
        .unwrap_err()
        .join(" ");
    assert!(error.contains("implementation steps"));
    assert!(error.contains("/1900"));
}

#[test]
fn single_string_in_a_story_list_field_is_normalized_to_one_entry() {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/task-story-1.json")).unwrap();
    value["task_stories"][0]["definition_of_done"] = "Story is complete and verified".into();
    let stories = decode_stories(&value.to_string()).unwrap();
    assert_eq!(
        stories[0].definition_of_done,
        ["Story is complete and verified"]
    );
}

#[test]
fn prose_without_a_story_json_is_not_repeated_as_repair_context() {
    let feedback = vec![
            "Invalid task story schema: response has no complete JSON object. The previous response contained no complete JSON story.".into(),
        ];
    let prior = previous_response_for_repair(&feedback, &"prose ".repeat(20_000));
    assert!(prior.contains("Return ONLY one complete JSON object"));
    assert!(
        prior.contains("first non-whitespace character must be {")
            && prior.contains("last non-whitespace character must be }")
    );

    let schema_feedback = vec!["The response is missing acceptance criteria".into()];
    assert_eq!(
        previous_response_for_repair(&schema_feedback, "partial story"),
        "partial story"
    );

    let budget_feedback = vec!["Field budget overages: implementation steps 2,900/1,900".into()];
    let repair = previous_response_for_repair(&budget_feedback, &"verbose draft ".repeat(10_000));
    assert!(repair.contains("rebuild the same single outline task"));
    assert!(repair.contains("one concrete runnable verification command"));
    assert!(!repair.contains("verification_commands to empty arrays"));
    assert!(!repair.contains("verbose draft"));
    assert!(repair_is_response_only(&budget_feedback));
    assert!(repair_is_response_only(&[
        "Invalid task story JSON: expected a comma".into()
    ]));
    assert!(!repair_is_response_only(&[
        "Story changed its approved scope references".into()
    ]));
}

#[test]
fn read_budget_failures_retry_as_response_only_repairs() {
    let feedback = vec![
        "read-only planning path limit exceeded: read requested src/core/state.rs for the 13th time".into(),
    ];
    let prior = previous_response_for_repair(&feedback, "partial JSON response");
    assert!(prior.contains("Do not call tools again"));
    assert!(repair_is_response_only(&feedback));
    assert_eq!(
        repair_execution_mode(crate::harness::ExecutionMode::TaskGeneration, &feedback),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
    assert_eq!(
        repair_execution_mode(
            crate::harness::ExecutionMode::TaskGeneration,
            &["Story changed its approved scope references".into()]
        ),
        crate::harness::ExecutionMode::TaskGeneration
    );
}

#[test]
fn multiple_story_envelope_failures_discard_the_prior_response() {
    let feedback = vec!["Return exactly one complete story in task_stories.".into()];
    let repair = previous_response_for_repair(&feedback, &"oversized response ".repeat(1_000));
    assert!(repair.contains("exactly one task_stories entry"));
    assert!(repair.contains("another placeholder story"));
    assert!(!repair.contains("oversized response"));
    assert!(repair_is_response_only(&feedback));
}

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
    let fixture = include_str!("../../../tests/fixtures/task-outline.json");
    let response =
        format!("I have enough context to outline the work; indexes are {{1, 2, 4}}.\n{fixture}");
    assert_eq!(decode_outline(&response).unwrap().len(), 2);
}

#[test]
fn scope_changes_are_rejected_but_wording_and_reference_order_are_stable() {
    let outlines =
        decode_outline(include_str!("../../../tests/fixtures/task-outline.json")).unwrap();
    let planned = &outlines[0];
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/task-story-1.json")).unwrap();
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
        decode_outline(include_str!("../../../tests/fixtures/task-outline.json")).unwrap();
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
        serde_json::from_str(include_str!("../../../tests/fixtures/task-story-1.json")).unwrap();
    value["task_stories"][0]["edge_cases"] = serde_json::json!([["nested"]]);
    let error = decode_stories(&value.to_string()).unwrap_err().join(" ");
    assert!(error.contains("list field contains a nested array"));
    assert!(error.contains("arrays of strings"));
}
