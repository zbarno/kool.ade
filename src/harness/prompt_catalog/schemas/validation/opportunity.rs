use super::super::{OpportunityConfidence, OpportunityProposalV1, OpportunityTarget};
use super::OutputValidationContext;

pub(super) fn validate(
    proposal: &OpportunityProposalV1,
    context: &OutputValidationContext<'_>,
) -> Result<(), String> {
    let nonempty = |value: &str, max: usize| {
        !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
    };
    let list_valid = |values: &[String], max_count: usize, max_value: usize| {
        values.len() <= max_count && values.iter().all(|value| nonempty(value, max_value))
    };
    let expected_identity = context
        .allowed_repositories
        .get(&proposal.target_repository_ref)
        .ok_or_else(|| {
            "opportunity target repository is not linked to this workspace".to_owned()
        })?;
    if proposal.target_repository != *expected_identity
        || !nonempty(&proposal.target_repository, 512)
        || !nonempty(&proposal.target_repository_ref, 128)
        || !nonempty(&proposal.observation_revision, 512)
        || !list_valid(&proposal.related_feature_uids, 128, 128)
        || !list_valid(&proposal.related_task_uids, 256, 128)
        || proposal.evidence_refs.is_empty()
        || !list_valid(&proposal.evidence_refs, 64, 256)
        || proposal
            .evidence_refs
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != proposal.evidence_refs.len()
        || !nonempty(&proposal.problem_or_opportunity, 4096)
        || !nonempty(&proposal.user_value, 4096)
        || !nonempty(&proposal.expected_outcome, 4096)
        || proposal.possible_approaches.is_empty()
        || !list_valid(&proposal.possible_approaches, 8, 4096)
        || !nonempty(&proposal.recommended_approach, 4096)
        || !nonempty(&proposal.alternatives, 4096)
        || !nonempty(&proposal.impact, 4096)
        || !nonempty(&proposal.cost_or_risk, 4096)
        || proposal.affected_systems.is_empty()
        || !list_valid(&proposal.affected_systems, 32, 256)
        || !nonempty(&proposal.current_capability_state, 4096)
        || proposal
            .generic_workaround
            .as_ref()
            .is_some_and(|value| !nonempty(value, 4096))
        || !nonempty(&proposal.proposed_approach, 4096)
        || proposal.dedup_fingerprint.len() != 64
        || !proposal
            .dedup_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !nonempty(&proposal.inference, 4096)
        || !nonempty(&proposal.generic_capability_check, 4096)
        || !proposal
            .evidence_refs
            .iter()
            .any(|reference| reference == "generic-capability-inventory")
        || !proposal
            .generic_capability_check
            .contains("generic-capability-inventory")
        || !nonempty(&proposal.deduplication_basis, 4096)
        || (proposal.speculative && matches!(proposal.confidence, OpportunityConfidence::High))
    {
        return Err("opportunity proposal is incomplete, inconsistent, or exceeds bounds".into());
    }

    let target_is_valid = match &proposal.target {
        OpportunityTarget::ActiveUserProject => {
            context.active_repository_ref.as_deref()
                == Some(proposal.target_repository_ref.as_str())
                && context.koolade_product_repository_ref.as_deref()
                    != Some(proposal.target_repository_ref.as_str())
        }
        OpportunityTarget::KooladeProduct => {
            context.koolade_product_repository_ref.as_deref()
                == Some(proposal.target_repository_ref.as_str())
        }
        OpportunityTarget::OtherLinkedRepository => {
            context.active_repository_ref.as_deref()
                != Some(proposal.target_repository_ref.as_str())
                && context.koolade_product_repository_ref.as_deref()
                    != Some(proposal.target_repository_ref.as_str())
        }
    };
    if !target_is_valid {
        return Err("opportunity target kind does not match the selected repository".into());
    }
    if proposal.evidence_refs.iter().any(|reference| {
        !context.evidence_ids.contains(reference)
            || (!matches!(&proposal.target, &OpportunityTarget::ActiveUserProject)
                && !context.public_evidence_ids.contains(reference))
    }) {
        return Err(
            "opportunity evidence is missing or not safe to share across repository boundaries"
                .into(),
        );
    }
    let typed_event_required = match &proposal.origin {
        super::super::OpportunityOrigin::RunFailure => Some("implementation-failure:"),
        super::super::OpportunityOrigin::QaFinding
        | super::super::OpportunityOrigin::ReviewFinding
        | super::super::OpportunityOrigin::SpecDrift => Some("lifecycle-event:"),
        _ => None,
    };
    if typed_event_required.is_some_and(|prefix| {
        !proposal
            .evidence_refs
            .iter()
            .any(|reference| reference.starts_with(prefix))
    }) {
        return Err("event-driven opportunity lacks its typed application evidence".into());
    }
    Ok(())
}
