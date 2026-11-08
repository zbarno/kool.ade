use super::{OutputValidationContext, RoleOutputV1, opportunity, spec_delta, task_graph};

pub(super) fn validate_output_refs(
    role: crate::harness::prompt_catalog::PromptRole,
    output: &RoleOutputV1,
    context: &OutputValidationContext<'_>,
) -> Result<(), String> {
    match output {
        RoleOutputV1::Manager(value) => {
            if context.require_grounded_manager_answer {
                if value.what_exists.trim().is_empty()
                    || value.how_known.is_empty()
                    || value.uncertainty.trim().is_empty()
                    || value.next_action.trim().is_empty()
                {
                    return Err("Manager answer must include what_exists, how_known, uncertainty, and next_action".into());
                }
                if value.what_exists.len() > 8_000
                    || value.uncertainty.len() > 4_000
                    || value.next_action.len() > 4_000
                    || value.how_known.len() > 16
                    || value.conflicts.len() > 8
                {
                    return Err("Manager answer exceeds its bounded response fields".into());
                }
                super::validate_refs(&value.how_known, &context.evidence_ids)?;
                for conflict in &value.conflicts {
                    if conflict.left_evidence_id == conflict.right_evidence_id
                        || conflict.claim.trim().is_empty()
                        || conflict.disposition.trim().is_empty()
                    {
                        return Err("Manager conflict must identify two sources, a claim, and a disposition".into());
                    }
                    super::validate_refs(
                        &[
                            conflict.left_evidence_id.clone(),
                            conflict.right_evidence_id.clone(),
                        ],
                        &context.evidence_ids,
                    )?;
                }
            }
            for conflict in &value.conflicts {
                super::validate_refs(
                    &[
                        conflict.left_evidence_id.clone(),
                        conflict.right_evidence_id.clone(),
                    ],
                    &context.evidence_ids,
                )?;
            }
            let broker_ids = context
                .evidence_ids
                .iter()
                .filter(|id| id.starts_with("investigation"))
                .collect::<std::collections::HashSet<_>>();
            if !broker_ids.is_empty() {
                if value.summary_evidence.is_empty()
                    || !value
                        .summary_evidence
                        .iter()
                        .any(|id| broker_ids.contains(id))
                {
                    return Err("Manager summary must cite verified investigation evidence".into());
                }
                super::validate_refs(&value.summary_evidence, &context.evidence_ids)?;
                let cited_text = value
                    .summary_evidence
                    .iter()
                    .filter_map(|id| context.evidence_content.get(id))
                    .flat_map(|text| text.lines())
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<std::collections::HashSet<_>>();
                if !cited_text.contains(value.summary.trim()) {
                    return Err(
                        "Manager summary must reproduce one verified evidence line verbatim".into(),
                    );
                }
            }
            for action in &value.proposed_actions {
                super::validate_refs(&action.evidence, &context.evidence_ids)?;
            }
            for proposal in &value.opportunities {
                super::validate_refs(&proposal.evidence_refs, &context.evidence_ids)?;
                opportunity::validate(proposal, context, role)?;
            }
        }
        RoleOutputV1::PlannerV3(value) => {
            super::validate_refs(&value.readiness.evidence, &context.evidence_ids)?;
            for question in &value.proposed_questions {
                super::validate_refs(&question.evidence_refs, &context.evidence_ids)?;
            }
            for resolution in &value.resolved_questions {
                if !context
                    .allowed_question_uids
                    .contains(&resolution.question_uid)
                {
                    return Err("planner resolved a question outside the current open set".into());
                }
                super::validate_refs(&resolution.evidence_refs, &context.evidence_ids)?;
            }
        }
        RoleOutputV1::Review(_) => {}
        RoleOutputV1::TaskGraph(graph) => task_graph::validate(graph, context)?,
        RoleOutputV1::WorkResult(_) => {}
        RoleOutputV1::SpecDelta(value) => {
            spec_delta::validate(value, context)?;
            for delta in &value.deltas {
                super::validate_refs(&delta.evidence, &context.evidence_ids)?;
            }
        }
        RoleOutputV1::PrProposal(value) => {
            super::validate_refs(&value.evidence, &context.evidence_ids)?
        }
        RoleOutputV1::OpportunityDecision(_) => {}
        RoleOutputV1::QaScenarioDraft(_) => {}
        RoleOutputV1::BootstrapConversation(_) => {}
        RoleOutputV1::PlanningTurn(_) => {}
    }
    Ok(())
}
