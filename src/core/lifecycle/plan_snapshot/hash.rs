use super::{PlanRequirementV1, PlanSnapshotV1, content::sha256};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotContent<'a> {
    schema_version: u16,
    feature_uid: &'a str,
    feature_id: &'a str,
    original_goal_ref: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_goal: Option<&'a str>,
    intent: &'a str,
    outcome: &'a str,
    current_state: &'a str,
    target_behavior: &'a str,
    scope: &'a [String],
    non_goals: &'a [String],
    requirements: &'a [PlanRequirementV1],
    acceptance_criteria: &'a [String],
    architecture: &'a str,
    dependencies: &'a [String],
    risks: &'a [String],
    security_constraints: &'a [String],
    test_strategy: &'a str,
    deployment_notes: &'a str,
    open_questions: &'a [String],
    deferred_assumptions: &'a [String],
    decisions: &'a str,
    spec_revision: &'a str,
    approval_contract_sha256: &'a str,
}

pub(super) fn content_hash(snapshot: &PlanSnapshotV1) -> Result<String, String> {
    let content = SnapshotContent {
        schema_version: snapshot.schema_version,
        feature_uid: &snapshot.feature_uid,
        feature_id: &snapshot.feature_id,
        original_goal_ref: &snapshot.original_goal_ref,
        original_goal: (snapshot.schema_version >= 2).then_some(&snapshot.original_goal),
        intent: &snapshot.intent,
        outcome: &snapshot.outcome,
        current_state: &snapshot.current_state,
        target_behavior: &snapshot.target_behavior,
        scope: &snapshot.scope,
        non_goals: &snapshot.non_goals,
        requirements: &snapshot.requirements,
        acceptance_criteria: &snapshot.acceptance_criteria,
        architecture: &snapshot.architecture,
        dependencies: &snapshot.dependencies,
        risks: &snapshot.risks,
        security_constraints: &snapshot.security_constraints,
        test_strategy: &snapshot.test_strategy,
        deployment_notes: &snapshot.deployment_notes,
        open_questions: &snapshot.open_questions,
        deferred_assumptions: &snapshot.deferred_assumptions,
        decisions: &snapshot.decisions,
        spec_revision: &snapshot.spec_revision,
        approval_contract_sha256: &snapshot.approval_contract_sha256,
    };
    serde_json::to_vec(&content)
        .map(|bytes| sha256(&bytes))
        .map_err(|error| error.to_string())
}
