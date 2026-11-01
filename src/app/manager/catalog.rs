use sha2::Digest;
use std::collections::{HashMap, HashSet};

use crate::harness::prompt_catalog::{
    EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, OutputValidationContext, PromptRole,
    RenderedPrompt, RoleInputV1, RolePayloadV1,
};

pub(in crate::app::manager) mod code_reviews;
mod decode;
mod opportunities;
mod payload;
mod product;
pub(in crate::app::manager) mod reviews;
pub(super) use decode::decode;
pub(super) use opportunities::persist_opportunities;
pub(super) use product::render_product;
pub(super) const KOOLADE_PRODUCT_TARGET_REF: &str = "koolade-product";

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;

pub(super) struct ManagerPrompt {
    pub rendered: RenderedPrompt,
    pub validation: OutputValidationContext<'static>,
    pub evidence_hashes: std::collections::HashMap<String, String>,
    pub evidence_confidentialities: std::collections::HashMap<String, EvidenceConfidentiality>,
    pub planning_revision: String,
    pub review_targets: std::collections::HashMap<String, reviews::ReviewTarget>,
    pub repo_root: Option<std::path::PathBuf>,
    pub _workspace_guard: Option<tempfile::TempDir>,
    pub code_review_targets: std::collections::HashMap<String, code_reviews::FindingTarget>,
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
    let original_goal = if original_goal.trim().is_empty() {
        "Provide an accurate update on current workspace activity.".into()
    } else {
        crate::core::context_build::clip_bytes(&original_goal, 16 * 1024)
    };
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
    let code_review_evidence = code_reviews::current_findings(project)?;
    let mut evidence_manifest = vec![EvidenceItem {
        id: "workspace-summary".into(),
        workspace_uid: workspace_uid.clone(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "sanitized current workspace activity".into(),
        content: summary,
    }];
    evidence_manifest.extend(trusted_evidence);
    let code_review_targets = code_review_evidence
        .iter()
        .map(|(item, target)| (item.id.clone(), target.clone()))
        .collect();
    evidence_manifest.extend(code_review_evidence.into_iter().map(|(item, _)| item));
    // Product discovery runs in its own product-only prompt and empty app-owned
    // cwd. A workspace Manager must never mix even public product evidence into
    // a user-project context.
    let public_evidence_ids = HashSet::new();
    let evidence_repository_refs = HashMap::new();
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
    let evidence_confidentialities = evidence_manifest
        .iter()
        .map(|item| (item.id.clone(), item.confidentiality))
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
            board_event_cursor: payload::event_cursor(events),
            work_graph: payload::work_graph(project),
            unresolved_findings: payload::unresolved_findings(project),
            latest_review_receipts: payload::latest_review_receipts(latest_review_receipts),
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
        evidence_confidentialities,
        planning_revision: project.state.baseline_planning_revision.clone(),
        review_targets,
        repo_root: None,
        _workspace_guard: None,
        code_review_targets,
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
                .target_if_available(
                    project.state.planning_store.project_id,
                    &project.state.repo_root,
                    &repository.id,
                )
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
