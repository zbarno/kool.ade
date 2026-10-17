use super::{OutputValidationContext, RoleOutputV1, opportunity, spec_delta, task_graph};

pub(super) fn validate_output_refs(
    output: &RoleOutputV1,
    context: &OutputValidationContext<'_>,
) -> Result<(), String> {
    match output {
        RoleOutputV1::Manager(value) => {
            for action in &value.proposed_actions {
                super::validate_refs(&action.evidence, &context.evidence_ids)?;
            }
            for proposal in &value.opportunities {
                super::validate_refs(&proposal.evidence_refs, &context.evidence_ids)?;
                opportunity::validate(proposal, context)?;
            }
        }
        RoleOutputV1::ManagerLegacy(value) => {
            for action in &value.proposed_actions {
                super::validate_refs(&action.evidence, &context.evidence_ids)?;
            }
        }
        RoleOutputV1::Planner(value) => {
            super::validate_refs(&value.readiness.evidence, &context.evidence_ids)?
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
        RoleOutputV1::PlanningTurn(_) => {}
    }
    Ok(())
}
