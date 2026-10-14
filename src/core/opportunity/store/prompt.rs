use crate::{
    artifacts::planning_store::StoreError, core::opportunity::OpportunityV1,
    harness::prompt_catalog::OpportunityProposalV1,
};

pub(super) fn validate_prompt_projection(
    normalized: &OpportunityV1,
    prompt: &OpportunityProposalV1,
) -> Result<(), StoreError> {
    let enum_label = |value: &dyn std::fmt::Debug| format!("{value:?}");
    if enum_label(&normalized.origin) != enum_label(&prompt.origin)
        || enum_label(&normalized.kind) != enum_label(&prompt.kind)
        || enum_label(&normalized.target.kind) != enum_label(&prompt.target)
        || enum_label(&normalized.estimated_scope) != enum_label(&prompt.estimated_scope)
        || normalized.problem_or_opportunity != prompt.problem_or_opportunity
        || normalized.user_value != prompt.user_value
        || normalized.expected_outcome != prompt.expected_outcome
        || normalized.recommended_approach != prompt.recommended_approach
        || normalized.proposed_approach != prompt.proposed_approach
        || normalized.impact != prompt.impact
        || normalized.cost_or_risk != prompt.cost_or_risk
        || normalized.current_capability_state != prompt.current_capability_state
        || normalized.generic_workaround != prompt.generic_workaround
        || normalized.affected_systems != prompt.affected_systems
        || normalized.possible_approaches != prompt.possible_approaches
        || normalized.observation_revision.to_string() != prompt.observation_revision
    {
        return Err(StoreError::MalformedState(
            "normalized opportunity does not match its advisory model proposal".into(),
        ));
    }
    Ok(())
}
