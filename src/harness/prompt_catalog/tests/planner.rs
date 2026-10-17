use super::*;

fn planner_v4_output() -> String {
    serde_json::json!({
        "proposedQuestions": [{
            "decisionSubject": "Authentication mode",
            "decisionDomain": "Security",
            "originRequirementIds": ["REQ-1"],
            "category": "Security",
            "priority": "Blocking",
            "question": "Which authentication mode should the feature use?",
            "reason": "The interface contract depends on identity guarantees.",
            "options": [
                {"id":"session", "label":"Session cookie", "tradeoffs":"Requires server state."},
                {"id":"token", "label":"Bearer token", "tradeoffs":"Requires token storage."}
            ],
            "recommendedOptionId": "session",
            "noAnswerImpact": "The authentication contract remains unresolved.",
            "evidenceRefs": ["e1"],
            "assumptionIfDeferred": null,
            "dependsOnQuestionIds": []
        }],
        "resolvedQuestions": [],
        "draftChanges": [],
        "readiness": {
            "ready": false,
            "blockers": ["Authentication mode is unresolved."],
            "evidence": ["e1"],
            "assessedAxes": ["security", "testing"],
            "acceptanceTests": [{"requirementId":"REQ-1", "testRef":"tests/auth_contract.rs"}]
        }
    })
    .to_string()
}

#[test]
fn planner_v4_accepts_typed_question_and_readiness_contract() {
    let output = validate_output_with_context(
        PromptRole::Planner,
        &planner_v4_output(),
        &OutputValidationContext {
            evidence_ids: ["e1".to_owned()].into(),
            required_requirements: vec!["REQ-1".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(output, RoleOutputV1::PlannerV3(_)));
}

#[test]
fn planner_v4_rejects_unknown_evidence_and_invalid_recommendation() {
    let mut output: serde_json::Value = serde_json::from_str(&planner_v4_output()).unwrap();
    output["proposedQuestions"][0]["evidenceRefs"] = serde_json::json!(["not-in-manifest"]);
    assert!(validate_output(PromptRole::Planner, &output.to_string(), None).is_err());

    output["proposedQuestions"][0]["evidenceRefs"] = serde_json::json!(["e1"]);
    output["proposedQuestions"][0]["recommendedOptionId"] = serde_json::json!("missing");
    assert!(validate_output(PromptRole::Planner, &output.to_string(), None).is_err());
}

#[test]
fn planner_v4_cannot_invent_requirement_ids_outside_the_active_plan() {
    let context = OutputValidationContext {
        evidence_ids: ["e1".to_owned()].into(),
        required_requirements: vec!["REQ-2".into()],
        ..Default::default()
    };
    assert!(
        validate_output_with_context(PromptRole::Planner, &planner_v4_output(), &context).is_err()
    );
}

#[test]
fn planner_v4_resolves_only_current_questions_with_manifest_evidence() {
    let uid = uuid::Uuid::new_v4().to_string();
    let mut output: serde_json::Value = serde_json::from_str(&planner_v4_output()).unwrap();
    output["resolvedQuestions"] = serde_json::json!([{
        "questionUid": uid.clone(),
        "decision": "Reuse the existing response envelope",
        "rationale": "The repository evidence documents the established interface.",
        "selectedOptionId": "session",
        "evidenceRefs": ["e1"]
    }]);
    let context = OutputValidationContext {
        evidence_ids: ["e1".to_owned()].into(),
        allowed_question_uids: [uid.clone()].into(),
        ..Default::default()
    };
    assert!(
        validate_output_with_context(PromptRole::Planner, &output.to_string(), &context).is_ok()
    );

    let invalid_context = OutputValidationContext {
        allowed_question_uids: Default::default(),
        ..context
    };
    assert!(
        validate_output_with_context(PromptRole::Planner, &output.to_string(), &invalid_context)
            .is_err()
    );
}
