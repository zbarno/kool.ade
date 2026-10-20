use crate::harness::prompt_catalog::{
    EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, ManagerActionKind, ManagerActionV1,
    ReviewOutcomeV1,
};

pub(in crate::app::manager) struct PlanReviewEvidence {
    pub id: String,
    pub item: EvidenceItem,
    pub supporting_items: Vec<EvidenceItem>,
    pub target: ReviewTarget,
}

#[derive(Clone)]
pub(crate) struct ReviewTarget {
    pub feature_id: String,
    pub revision: String,
    pub finding_ids: Vec<String>,
    pub finding_claims: std::collections::HashMap<String, String>,
    pub evidence_ids: Vec<String>,
}

pub(in crate::app::manager) fn current_review_evidence(
    project: &super::super::super::session::Project,
) -> Vec<PlanReviewEvidence> {
    project
        .state
        .active_features
        .iter()
        .filter_map(|(feature_id, markdown)| {
            let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()?;
            let contract = crate::core::workflow::feature_contract(markdown);
            let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
                &project.state.planning_store,
                feature_id,
                &identity,
                markdown,
                &contract,
            )
            .ok()?;
            let verdict = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
                &project.state.planning_store,
                &snapshot,
            )
            .ok()
            .flatten()?;
            let needs_manager =
                !verdict.pass || verdict.outcome == Some(ReviewOutcomeV1::ReviewUnable);
            if !needs_manager {
                return None;
            }
            let id = format!("plan-review:{}", snapshot.uid);
            let content = serde_json::to_string(&verdict).ok()?;
            let supporting =
                crate::app::root::plan_review::evidence::collect(project, &snapshot).ok()?;
            let supporting_items = supporting.items;
            Some(PlanReviewEvidence {
                id: id.clone(),
                item: EvidenceItem {
                    id,
                    workspace_uid: project
                        .state
                        .planning_store
                        .project_id
                        .hyphenated()
                        .to_string(),
                    origin: EvidenceOrigin::Application,
                    confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                    source: format!("persisted independent plan review for {feature_id}"),
                    content,
                },
                target: ReviewTarget {
                    feature_id: feature_id.clone(),
                    revision: snapshot.content_hash,
                    finding_ids: verdict
                        .findings
                        .iter()
                        .map(|finding| finding.id.clone())
                        .collect(),
                    finding_claims: verdict
                        .findings
                        .iter()
                        .map(|finding| (finding.id.clone(), canonical_finding_claim(finding)))
                        .collect(),
                    evidence_ids: supporting_items
                        .iter()
                        .map(|item| item.id.clone())
                        .collect(),
                },
                supporting_items,
            })
        })
        .collect()
}

pub(super) fn validate_dispositions(
    actions: &[ManagerActionV1],
    reviews: &std::collections::HashMap<String, ReviewTarget>,
    available_evidence: &std::collections::HashSet<String>,
    evidence_content: &std::collections::HashMap<String, String>,
) -> Result<(), String> {
    use ManagerActionKind::{AskUser, ContestFinding, PromoteFollowUp, RepairPlan};
    let is_disposition = |kind: &ManagerActionKind| {
        matches!(
            kind,
            RepairPlan | AskUser | PromoteFollowUp | ContestFinding
        )
    };
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for action in actions.iter().filter(|action| is_disposition(&action.kind)) {
        let referenced = action
            .evidence
            .iter()
            .filter_map(|id| reviews.get(id).map(|review| (id, review)))
            .collect::<Vec<_>>();
        if referenced.len() != 1 {
            return Err(
                "plan-review disposition must cite exactly one current review record".into(),
            );
        }
        let (review_id, review) = referenced[0];
        if action.evidence.iter().any(|id| {
            !available_evidence.contains(id)
                || (id != review_id && !review.evidence_ids.iter().any(|source| source == id))
        }) {
            return Err(
                "plan-review disposition must cite only its current review and supporting evidence"
                    .into(),
            );
        }
        let Some(finding_id) = action
            .target
            .strip_prefix(&format!("{}#", review.feature_id))
        else {
            return Err("plan-review disposition target must name its feature and finding".into());
        };
        if !review.finding_ids.iter().any(|id| id == finding_id)
            || action.expected_revision != review.revision
        {
            return Err("plan-review disposition refers to a stale or unknown finding".into());
        }
        if matches!(&action.kind, ContestFinding) {
            let expected_claim = review.finding_claims.get(finding_id);
            let independent_sources = action
                .evidence
                .iter()
                .filter(|id| {
                    available_evidence.contains(*id)
                        && review.evidence_ids.iter().any(|evidence| evidence == *id)
                        && [
                            "plan:",
                            "decision:",
                            "feature-decisions:",
                            "readiness:",
                            "product-spec:",
                        ]
                        .iter()
                        .any(|prefix| id.starts_with(prefix))
                })
                .collect::<std::collections::HashSet<_>>();
            let grounded = action.contradiction.as_ref().is_some_and(|claim| {
                independent_sources.contains(&&claim.source_evidence_id)
                    && !claim.finding_claim.trim().is_empty()
                    && expected_claim.is_some_and(|expected| claim.finding_claim.trim() == expected)
                    && !claim.explanation.trim().is_empty()
                    && claim.explanation.trim().len() >= 32
                    && claim.source_quote.trim().len() >= 8
                    && evidence_content
                        .get(&claim.source_evidence_id)
                        .is_some_and(|content| content.contains(&claim.source_quote))
            });
            if independent_sources.is_empty() || action.reason.trim().len() < 32 || !grounded {
                return Err(
                    "contesting a plan finding requires its exact persisted claim and a contradiction grounded in distinct plan, decision, readiness, or product-spec evidence".into(),
                );
            }
        }
        *seen.entry(format!("{review_id}#{finding_id}")).or_default() += 1;
    }
    for (review_id, review) in reviews {
        for finding_id in &review.finding_ids {
            if seen.get(&format!("{review_id}#{finding_id}")) != Some(&1) {
                return Err(format!(
                    "Manager must return exactly one disposition for finding {finding_id}"
                ));
            }
        }
    }
    Ok(())
}

fn canonical_finding_claim(finding: &crate::harness::prompt_catalog::ReviewFindingV1) -> String {
    format!(
        "{}: {}",
        finding.location.trim(),
        finding.proposed_correction.trim()
    )
}

#[cfg(test)]
#[path = "reviews/tests.rs"]
mod tests;
