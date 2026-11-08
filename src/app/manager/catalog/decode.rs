use super::{code_reviews, reviews};
use crate::app::manager::ManagerOutput;
use crate::harness::prompt_catalog::{
    EvidenceConfidentiality, OutputValidationContext, PromptRole, RoleOutputV1,
};

pub(in crate::app::manager) fn decode(
    text: &str,
    context: &OutputValidationContext<'_>,
    evidence_hashes: std::collections::HashMap<String, String>,
    evidence_confidentialities: std::collections::HashMap<String, EvidenceConfidentiality>,
    planning_revision: String,
    review_targets: &std::collections::HashMap<String, reviews::ReviewTarget>,
    code_review_targets: &std::collections::HashMap<String, code_reviews::FindingTarget>,
) -> Result<ManagerOutput, String> {
    let RoleOutputV1::Manager(output) =
        crate::harness::prompt_catalog::validate_output_with_context(
            PromptRole::Manager,
            text,
            context,
        )?
    else {
        return Err("manager returned the wrong output schema".into());
    };
    reviews::validate_dispositions(
        &output.proposed_actions,
        review_targets,
        &context.evidence_ids,
        &context.evidence_content,
    )?;
    code_reviews::validate_dispositions(
        &output.proposed_actions,
        code_review_targets,
        &context.evidence_ids,
    )?;
    let uncertainty_reported =
        crate::app::manager::output::uncertainty_reported(&output.uncertainty);
    let proposed_actions = output.proposed_actions;
    let mut lines = vec![output.summary];
    lines.push(format!("What exists: {}", output.what_exists));
    lines.push(format!("How known: {}", output.how_known.join(", ")));
    lines.push(format!("Uncertainty: {}", output.uncertainty));
    lines.push(format!("Next action: {}", output.next_action));
    for conflict in &output.conflicts {
        lines.push(format!(
            "Evidence conflict: {} vs {} — {} ({})",
            conflict.left_evidence_id,
            conflict.right_evidence_id,
            conflict.claim,
            conflict.disposition
        ));
    }
    if !output.summary_evidence.is_empty() {
        let evidence = output
            .summary_evidence
            .iter()
            .filter_map(|id| {
                evidence_hashes
                    .get(id)
                    .map(|hash| format!("{id} sha256:{hash}"))
            })
            .collect::<Vec<_>>();
        if !evidence.is_empty() {
            lines.push(format!("Evidence: {}", evidence.join(", ")));
        }
    }
    for action in &proposed_actions {
        lines.push(format!(
            "{:?}: {} — {}",
            action.kind, action.target, action.reason
        ));
    }
    for opportunity in &output.opportunities {
        lines.push(format!(
            "Proposed improvement for {}: {} — {} (target repository {})",
            opportunity.target_repository_ref,
            opportunity.problem_or_opportunity,
            opportunity.user_value,
            opportunity.target_repository
        ));
    }
    Ok(ManagerOutput {
        summary: lines.join("\n"),
        uncertainty_reported,
        proposed_actions,
        opportunities: output.opportunities,
        evidence_hashes,
        evidence_confidentialities,
        planning_revision,
        code_review_targets: code_review_targets.clone(),
    })
}
