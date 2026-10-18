use crate::harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin};

pub(super) struct ReviewerContext {
    pub original_goal: String,
    pub evidence_manifest: Vec<EvidenceItem>,
    pub evidence_ids: Vec<String>,
    pub decision_refs: Vec<String>,
}

pub(super) fn build(
    project: &super::super::super::session::Project,
    feature_id: &str,
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
) -> Result<ReviewerContext, String> {
    if let Some(requirement) = snapshot.requirements.iter().find(|requirement| {
        requirement
            .rationale
            .as_deref()
            .is_none_or(|rationale| rationale.trim().is_empty())
            || requirement.ac_ids.is_empty()
    }) {
        return Err(format!(
            "requirement {} needs an explicit rationale and acceptance-criteria mapping before independent review",
            requirement.stable_id
        ));
    }
    let original_work = project
        .planning_work
        .iter()
        .find(|work| {
            work.kind == crate::core::planning_work::WorkKind::Feature
                && work.feature_id.as_deref() == Some(feature_id)
                && work.feature_uid.as_deref() == Some(snapshot.feature_uid.as_str())
        })
        .ok_or_else(|| {
            "the initiating feature work is unavailable; independent review is blocked".to_owned()
        })?;
    let original_goal = original_work.request.as_str();
    if original_goal.trim().is_empty() {
        return Err(
            "the initiating user goal is unavailable; independent review is blocked".into(),
        );
    }
    let original_goal_ref = format!(
        "planning_work:{}:request_sha256:{:x}",
        original_work.uid,
        sha2::Sha256::digest(original_goal.as_bytes())
    );
    if snapshot.original_goal_ref != original_goal_ref {
        return Err(
            "the initiating user goal reference does not match this plan snapshot; independent review is blocked".into(),
        );
    }
    let product_spec =
        crate::artifacts::product_docs::render_product(&project.state.planning_store)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                "the current product specification is unavailable for independent review".to_owned()
            })?;
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let product_hash = format!("{:x}", sha2::Sha256::digest(product_spec.as_bytes()));
    let plan_evidence_id = format!("plan:{}", snapshot.uid);
    let goal_evidence_id = format!("goal:{:x}", sha2::Sha256::digest(original_goal.as_bytes()));
    let product_evidence_id = format!("product-spec:{product_hash}");
    let mut evidence_manifest = vec![
        EvidenceItem {
            id: plan_evidence_id.clone(),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("immutable approved plan snapshot {}", snapshot.uid),
            content: serde_json::to_string(snapshot).map_err(|error| error.to_string())?,
        },
        EvidenceItem {
            id: goal_evidence_id.clone(),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::User,
            confidentiality: EvidenceConfidentiality::Restricted,
            source: format!("initiating user request for feature {feature_id}"),
            content: original_goal.to_owned(),
        },
        EvidenceItem {
            id: product_evidence_id.clone(),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("current product specification sha256 {product_hash}"),
            content: product_spec,
        },
    ];
    let requirement_evidence = snapshot
        .requirements
        .iter()
        .map(|requirement| {
            Ok(EvidenceItem {
                id: format!("requirement:{}", requirement.stable_id),
                workspace_uid: workspace_uid.clone(),
                origin: EvidenceOrigin::Application,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: format!("approved plan requirement {}", requirement.stable_id),
                content: serde_json::to_string(requirement).map_err(|error| error.to_string())?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut evidence_ids = vec![
        plan_evidence_id,
        goal_evidence_id,
        product_evidence_id.clone(),
    ];
    evidence_ids.extend(requirement_evidence.iter().map(|item| item.id.clone()));
    evidence_manifest.extend(requirement_evidence);
    Ok(ReviewerContext {
        original_goal: original_goal.to_owned(),
        evidence_manifest,
        evidence_ids,
        decision_refs: vec![format!("feature:{feature_id}"), product_evidence_id],
    })
}

use sha2::Digest;
