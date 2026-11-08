use super::*;
use std::collections::HashMap;

#[path = "opportunity/golden.rs"]
mod golden;
#[path = "opportunity/targets.rs"]
mod targets;
#[path = "opportunity/validation.rs"]
mod validation;

fn proposal() -> OpportunityProposalV1 {
    OpportunityProposalV1 {
        origin: OpportunityOrigin::CodeInspection,
        kind: OpportunityKind::Quality,
        target: OpportunityTarget::ActiveUserProject,
        target_repository: "https://github.com/example/project".into(),
        target_repository_ref: "project".into(),
        related_feature_uids: vec![],
        related_task_uids: vec![],
        observation_revision: "a".repeat(64),
        evidence_refs: vec![
            "workspace-summary".into(),
            "generic-capability-inventory".into(),
        ],
        problem_or_opportunity: "Repeated verification failures cost time".into(),
        user_value: "Faster, more reliable iteration".into(),
        expected_outcome: "Fewer repeated failures".into(),
        possible_approaches: vec!["Improve preflight diagnostics".into()],
        recommended_approach: "Add a deterministic preflight check".into(),
        alternatives: "Keep current diagnostics".into(),
        estimated_scope: OpportunityScope::Small,
        impact: "Shorter repair cycles".into(),
        cost_or_risk: "Small maintenance cost".into(),
        affected_systems: vec!["verification".into()],
        current_capability_state: "Generic verification exists but lacks this check".into(),
        generic_workaround: Some("Inspect the task log manually".into()),
        proposed_approach: "Add the preflight check to the existing flow".into(),
        dedup_fingerprint: "b".repeat(64),
        inference: "Repeated failures suggest a recurring diagnostic gap".into(),
        confidence: OpportunityConfidence::Medium,
        speculative: false,
        generic_capability_check:
            "Checked generic-capability-inventory: the current generic path was inspected".into(),
        deduplication_basis: "No matching open proposal was supplied".into(),
    }
}

fn context() -> OutputValidationContext<'static> {
    OutputValidationContext {
        evidence_ids: [
            "workspace-summary".to_owned(),
            "generic-capability-inventory".to_owned(),
        ]
        .into(),
        allowed_repositories: HashMap::from([(
            "project".to_owned(),
            "https://github.com/example/project".to_owned(),
        )]),
        active_repository_ref: Some("project".into()),
        public_evidence_ids: ["workspace-summary".to_owned()].into(),
        ..Default::default()
    }
}

#[test]
fn grounded_manager_answer_requires_complete_fields_and_manifest_citations() {
    let evidence_id = "repository-architecture-profile";
    let context = OutputValidationContext {
        require_grounded_manager_answer: true,
        evidence_ids: [evidence_id.to_owned()].into(),
        ..Default::default()
    };
    let complete = serde_json::json!({
        "summary": "A bounded repository profile was found.",
        "what_exists": "A revision-pinned repository profile is available.",
        "how_known": [evidence_id],
        "uncertainty": "It may lag the current source revision.",
        "next_action": "Investigate the selected current revision for an exact code question.",
        "conflicts": [],
        "proposed_actions": [],
        "opportunities": []
    })
    .to_string();
    assert!(validate_output_with_context(PromptRole::Manager, &complete, &context).is_ok());

    let missing_citation = serde_json::json!({
        "summary": "A claim without a source.",
        "what_exists": "A feature exists.",
        "how_known": ["invented-evidence"],
        "uncertainty": "Unknown.",
        "next_action": "Check it.",
        "conflicts": [],
        "proposed_actions": [],
        "opportunities": []
    })
    .to_string();
    assert!(
        validate_output_with_context(PromptRole::Manager, &missing_citation, &context).is_err()
    );
}

fn manager_output(proposal: &OpportunityProposalV1) -> String {
    serde_json::json!({
        "summary": "A small improvement may reduce recurring failures.",
        "proposed_actions": [],
        "opportunities": [proposal],
    })
    .to_string()
}

#[test]
fn manager_summary_must_cite_verified_investigation_evidence() {
    let evidence_id = "investigation:00000000-0000-4000-8000-000000000001:src/lib.rs";
    let evidence_line =
        "Untrusted repository evidence at src/lib.rs: pub fn parse(input: &str) -> Result<()>";
    let mut context = context();
    context.evidence_ids.insert(evidence_id.into());
    context
        .evidence_content
        .insert(evidence_id.into(), evidence_line.into());
    let without_citation = serde_json::json!({
        "summary": "The parser handles nested input.",
        "proposed_actions": [],
        "opportunities": []
    })
    .to_string();
    assert!(
        validate_output_with_context(PromptRole::Manager, &without_citation, &context).is_err()
    );
    let cited = serde_json::json!({
        "summary": evidence_line,
        "summary_evidence": [evidence_id],
        "proposed_actions": [],
        "opportunities": []
    })
    .to_string();
    assert!(validate_output_with_context(PromptRole::Manager, &cited, &context).is_ok());
    let unknown = serde_json::json!({
        "summary": evidence_line,
        "summary_evidence": ["investigation:unknown:src/lib.rs"],
        "proposed_actions": [],
        "opportunities": []
    })
    .to_string();
    assert!(validate_output_with_context(PromptRole::Manager, &unknown, &context).is_err());
    let unsupported = serde_json::json!({
        "summary": "The parser is safe against all malformed input.",
        "summary_evidence": [evidence_id],
        "proposed_actions": [],
        "opportunities": []
    })
    .to_string();
    assert!(validate_output_with_context(PromptRole::Manager, &unsupported, &context).is_err());
}
