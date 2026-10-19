use sha2::Digest;
use std::collections::{HashMap, HashSet};

use super::ManagerOutput;
use crate::harness::prompt_catalog::{
    EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, OutputValidationContext, PromptRole,
    RenderedPrompt, RoleInputV1, RoleOutputV1, RolePayloadV1,
};

mod opportunities;
pub(in crate::app::manager) mod reviews;
pub(super) use opportunities::persist_opportunities;

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;

pub(super) struct ManagerPrompt {
    pub rendered: RenderedPrompt,
    pub validation: OutputValidationContext<'static>,
    pub evidence_hashes: std::collections::HashMap<String, String>,
    pub planning_revision: String,
    pub review_targets: std::collections::HashMap<String, reviews::ReviewTarget>,
}

pub(super) fn render(
    project: &super::super::session::Project,
    events: &[String],
) -> Result<ManagerPrompt, String> {
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let feature_uid = project
        .state
        .active_feature
        .as_ref()
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| workspace_uid.clone());
    let original_goal = project
        .chat
        .iter()
        .rev()
        .find(|message| message.role == crate::domain::chatlog::ChatRole::User)
        .map(|message| crate::core::context_build::clip(&message.text, 4000))
        .or_else(|| {
            project
                .state
                .active_feature
                .as_ref()
                .map(|(_, text)| crate::core::workflow::feature_contract(text))
        })
        .unwrap_or_else(|| "Provide an accurate update on current workspace activity.".into());
    let approved_spec = project.state.active_feature.as_ref().filter(|(id, _)| {
        crate::core::workflow::feature_approved(
            &project.state.planning_store,
            &project.state.workflow,
            id,
        )
    });
    let koolade_product_repository_ref = project
        .state
        .repositories
        .repositories
        .iter()
        .find(|repository| is_koolade_product_remote(&repository.remote))
        .map(|repository| repository.id.clone());
    let summary = super::Manager::prompt_body(project, events);
    let trusted_evidence = opportunities::trusted_opportunity_evidence(project);
    let plan_review_evidence = reviews::current_review_evidence(project);
    let mut evidence_manifest = vec![EvidenceItem {
        id: "workspace-summary".into(),
        workspace_uid: workspace_uid.clone(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "sanitized current workspace activity".into(),
        content: summary,
    }];
    evidence_manifest.extend(trusted_evidence);
    let mut public_evidence_ids = HashSet::new();
    let mut evidence_repository_refs = HashMap::new();
    if let Some(repository_ref) = &koolade_product_repository_ref {
        let product_evidence = opportunities::koolade_product_contract_evidence(&workspace_uid);
        public_evidence_ids.insert(product_evidence.id.clone());
        evidence_repository_refs.insert(product_evidence.id.clone(), repository_ref.clone());
        evidence_manifest.push(product_evidence);
    }
    let latest_review_receipts = plan_review_evidence
        .iter()
        .map(|review| review.id.clone())
        .collect();
    let review_targets = plan_review_evidence
        .iter()
        .map(|review| {
            evidence_manifest.push(review.item.clone());
            evidence_manifest.extend(review.supporting_items.iter().cloned());
            (
                review.id.clone(),
                reviews::ReviewTarget {
                    feature_id: review.target.feature_id.clone(),
                    revision: review.target.revision.clone(),
                    finding_ids: review.target.finding_ids.clone(),
                    finding_claims: review.target.finding_claims.clone(),
                    evidence_ids: review.target.evidence_ids.clone(),
                },
            )
        })
        .collect();
    evidence_manifest = deduplicate_evidence(evidence_manifest)?;
    let evidence_ids = evidence_manifest
        .iter()
        .map(|item| item.id.clone())
        .collect::<HashSet<_>>();
    let evidence_hashes = evidence_manifest
        .iter()
        .map(|item| {
            (
                item.id.clone(),
                format!("{:x}", sha2::Sha256::digest(item.content.as_bytes())),
            )
        })
        .collect::<HashMap<_, _>>();
    let evidence_content = evidence_manifest
        .iter()
        .map(|item| (item.id.clone(), item.content.clone()))
        .collect();
    let allowed_repositories = project
        .state
        .repositories
        .repositories
        .iter()
        .map(|repository| {
            let identity = if repository.remote.trim().is_empty() {
                repository
                    .display_name
                    .clone()
                    .unwrap_or_else(|| repository.id.clone())
            } else {
                repository.remote.clone()
            };
            (repository.id.clone(), identity)
        })
        .collect();
    let active_repository_ref = active_repository_ref(project);
    let input = RoleInputV1 {
        role: PromptRole::Manager,
        workspace_uid: workspace_uid.clone(),
        feature_uid,
        task_uid: None,
        original_goal,
        approved_spec_ref: approved_spec.map(|(id, _)| format!("feature:{id}")),
        approved_spec_hash: approved_spec.map(|(_, text)| {
            let contract = crate::core::workflow::feature_contract(text);
            format!("{:x}", sha2::Sha256::digest(contract.as_bytes()))
        }),
        decision_refs: Vec::new(),
        evidence_manifest,
        execution_policy_ref: "manager-policy/application-actions/v1".into(),
        output_schema_version: PromptRole::Manager.output_schema_version().into(),
        payload: RolePayloadV1::Manager {
            board_event_cursor: events.last().cloned().unwrap_or_default(),
            work_graph: project.active_implementations.keys().cloned().collect(),
            unresolved_findings: project
                .queue
                .blocked
                .iter()
                .map(|(task, issue)| format!("{task}: {}", issue.message))
                .collect(),
            latest_review_receipts,
        },
    };
    Ok(ManagerPrompt {
        rendered: crate::harness::prompt_catalog::render(&input)?,
        validation: OutputValidationContext {
            evidence_ids,
            evidence_content,
            allowed_repositories,
            active_repository_ref,
            koolade_product_repository_ref,
            public_evidence_ids,
            evidence_repository_refs,
            ..Default::default()
        },
        evidence_hashes,
        planning_revision: project.state.baseline_planning_revision.clone(),
        review_targets,
    })
}

fn deduplicate_evidence(items: Vec<EvidenceItem>) -> Result<Vec<EvidenceItem>, String> {
    let mut seen = HashMap::<String, EvidenceItem>::new();
    let mut unique = Vec::with_capacity(items.len());
    for item in items {
        if let Some(existing) = seen.get(&item.id) {
            if existing != &item {
                return Err(format!(
                    "Manager evidence ID {} resolved to conflicting content",
                    item.id
                ));
            }
            continue;
        }
        seen.insert(item.id.clone(), item.clone());
        unique.push(item);
    }
    Ok(unique)
}

fn active_repository_ref(project: &super::super::session::Project) -> Option<String> {
    let active_path = project.state.repo_root.canonicalize().ok()?;
    project
        .state
        .repositories
        .repositories
        .iter()
        .find(|repository| {
            project
                .state
                .repositories
                .target_if_available(&project.state.repo_root, &repository.id)
                .ok()
                .flatten()
                .and_then(|path| path.canonicalize().ok())
                .is_some_and(|path| path == active_path)
        })
        .map(|repository| repository.id.clone())
}

fn is_koolade_product_remote(remote: &str) -> bool {
    let remote = remote.trim().trim_end_matches(".git").to_ascii_lowercase();
    matches!(
        remote.as_str(),
        "https://github.com/zbarno/kool.ade"
            | "http://github.com/zbarno/kool.ade"
            | "git@github.com:zbarno/kool.ade"
            | "ssh://git@github.com/zbarno/kool.ade"
    )
}

pub(super) fn decode(
    text: &str,
    context: &OutputValidationContext<'_>,
    evidence_hashes: std::collections::HashMap<String, String>,
    planning_revision: String,
    review_targets: &std::collections::HashMap<String, reviews::ReviewTarget>,
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
    let proposed_actions = output.proposed_actions;
    let mut lines = vec![output.summary];
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
        proposed_actions,
        opportunities: output.opportunities,
        evidence_hashes,
        planning_revision,
    })
}
