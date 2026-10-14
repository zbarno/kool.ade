use super::OutputValidationContext;
use crate::harness::prompt_catalog::TaskGraphV1;
use std::collections::HashSet;

pub(super) fn validate(
    graph: &TaskGraphV1,
    context: &OutputValidationContext<'_>,
) -> Result<(), String> {
    graph.validate()?;
    let Some(expected_feature_uid) = context.expected_feature_uid else {
        return Err("task graph validation requires an application-bound feature identity".into());
    };
    if expected_feature_uid != graph.feature_uid {
        return Err(
            "task graph feature identity does not match the active approved feature".into(),
        );
    }
    if context.expected_approved_spec_ref != Some(graph.approved_spec_ref.as_str())
        || context.expected_approved_plan_hash != Some(graph.approved_plan_hash.as_str())
        || context
            .approved_decision_refs
            .iter()
            .collect::<HashSet<_>>()
            != graph.decision_refs.iter().collect::<HashSet<_>>()
    {
        return Err(
            "task graph plan snapshot or decision receipts differ from approved context".into(),
        );
    }
    if context.required_requirements.is_empty() {
        return Err("task graph validation requires the approved requirement set".into());
    }
    let expected = context.required_requirements.iter().collect::<HashSet<_>>();
    let actual = graph
        .approved_requirement_ids
        .iter()
        .collect::<HashSet<_>>();
    if expected != actual {
        return Err("task graph requirement set differs from the approved requirements".into());
    }
    if context.allowed_repositories.is_empty() || context.approved_branch_targets.is_empty() {
        return Err("task graph validation requires trusted repository and branch targets".into());
    }
    for task in &graph.tasks {
        if !context
            .allowed_repositories
            .contains_key(&task.repository_id)
            || context
                .approved_branch_targets
                .get(&task.repository_id)
                .is_none_or(|(source, destination)| {
                    source != &task.source_branch || destination != &task.destination_branch
                })
            || task.source_sha.as_ref() != context.repository_source_shas.get(&task.repository_id)
        {
            return Err(
                "task graph repository or source SHA is outside approved repository context".into(),
            );
        }
    }
    Ok(())
}
