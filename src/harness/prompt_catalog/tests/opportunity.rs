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

fn manager_output(proposal: &OpportunityProposalV1) -> String {
    serde_json::json!({
        "summary": "A small improvement may reduce recurring failures.",
        "proposed_actions": [],
        "opportunities": [proposal],
    })
    .to_string()
}
