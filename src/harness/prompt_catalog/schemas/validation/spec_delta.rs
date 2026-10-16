use super::super::{OutputValidationContext, SpecDeltaV1};
use std::collections::HashSet;

pub(super) fn validate(
    proposal: &SpecDeltaV1,
    context: &OutputValidationContext<'_>,
) -> Result<(), String> {
    if proposal.deltas.len() > 128 {
        return Err("specification delta output exceeds 128 entries".into());
    }
    if !proposal.deltas.is_empty() && context.required_requirements.is_empty() {
        return Err("nonempty specification deltas require the approved requirement set".into());
    }
    let mut seen = HashSet::new();
    for delta in &proposal.deltas {
        if delta.requirement_id.trim().is_empty()
            || delta.requirement_id.len() > 128
            || !seen.insert(&delta.requirement_id)
            || !context
                .required_requirements
                .contains(&delta.requirement_id)
            || delta.old.trim().is_empty()
            || delta.new.trim().is_empty()
            || delta.old.len() > 8192
            || delta.new.len() > 8192
            || delta.old == delta.new
            || delta.evidence.is_empty()
            || delta.followup_task_ids.len() > 64
            || delta
                .followup_task_ids
                .iter()
                .any(|uid| uid.trim().is_empty() || uid.len() > 128)
        {
            return Err(
                "specification output must contain unique, bounded, evidence-linked requirement deltas"
                    .into(),
            );
        }
    }
    Ok(())
}
