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
    let required_capability_evidence = match &proposal.target {
        OpportunityTarget::KooladeProduct => "koolade-product-opportunity-contract",
        _ => "generic-capability-inventory",
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
            .any(|reference| reference == required_capability_evidence)
        || !proposal
            .generic_capability_check
            .contains(required_capability_evidence)
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
        context
            .evidence_repository_refs
            .get(reference)
            .is_some_and(|repository| repository != &proposal.target_repository_ref)
    }) {
        return Err("opportunity evidence is scoped to a different repository".into());
    }
    let active_target = matches!(&proposal.target, OpportunityTarget::ActiveUserProject);
    let user_context_required = matches!(
        &proposal.origin,
        super::super::OpportunityOrigin::UserGoal | super::super::OpportunityOrigin::UserFeedback
    ) || (active_target
        && matches!(&proposal.origin, super::super::OpportunityOrigin::Roadmap));
    if user_context_required
        && (!active_target
            || !proposal
                .evidence_refs
                .iter()
                .any(|id| id == "workspace-summary"))
    {
        return Err("user-goal, feedback, and active-project roadmap proposals need current workspace evidence".into());
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
        super::super::OpportunityOrigin::RunFailure => Some(
            &[
                "implementation-failure:",
                "broker-prerequisite:",
                "provider-capability-mismatch:",
                "telemetry-regression:",
            ][..],
        ),
        super::super::OpportunityOrigin::QaFinding
        | super::super::OpportunityOrigin::ReviewFinding => Some(&["lifecycle-event:"][..]),
        super::super::OpportunityOrigin::SpecDrift => {
            Some(&["lifecycle-event:", "spec-finding:"][..])
        }
        _ => None,
    };
    if !matches!(&proposal.target, OpportunityTarget::ActiveUserProject)
        && typed_event_required.is_some()
    {
        return Err("private event origins cannot be proposed against another repository".into());
    }
    if typed_event_required.is_some_and(|prefixes| {
        !proposal
            .evidence_refs
            .iter()
            .any(|reference| prefixes.iter().any(|prefix| reference.starts_with(prefix)))
    }) {
        return Err("event-driven opportunity lacks its typed application evidence".into());
    }
    Ok(())
}
