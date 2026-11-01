use crate::{
    app::session::Project,
    core::planning_work::{Work, WorkKind, WorkStatus},
    harness::prompt_catalog::ManagerActionV1,
};
mod persist;
pub(crate) use persist::persist_dispositions;
#[path = "dispositions/code_reviews.rs"]
mod code_reviews;
#[path = "dispositions/fields.rs"]
mod fields;
#[path = "dispositions/kind.rs"]
mod kind;
#[path = "dispositions/qa_findings.rs"]
mod qa_findings;
pub(crate) use code_reviews::approve_risk as approve_code_review_risk;
pub(crate) use code_reviews::finish_repair as finish_code_review_repair;
pub(crate) use code_reviews::has_unpromoted_nonblocking_findings;
pub(crate) use code_reviews::mark_repair_failed as mark_code_review_repair_failed;
pub(crate) use code_reviews::next_repair as next_code_review_repair;
pub(crate) use code_reviews::{
    CodeReviewArbitrationRequest, finish_arbitration as finish_code_review_arbitration,
    next_arbitration as next_code_review_arbitration,
};
use kind::DispositionWork;
#[cfg(test)]
pub(crate) use qa_findings::next_repair_with_store_for_test as next_qa_finding_repair_with_store;
pub(crate) use qa_findings::{
    finish_after_qa as finish_qa_finding_repair, next_repair as next_qa_finding_repair,
};
use sha2::Digest;

/// Persist validated Manager decisions as ordinary planning work, using a
/// stable candidate/finding identity so repeated Manager runs cannot duplicate cards.
#[cfg(test)]
pub fn persist_plan_dispositions(
    project: &mut Project,
    actions: &[ManagerActionV1],
    expected_evidence_hashes: &std::collections::HashMap<String, String>,
    expected_planning_revision: &str,
) -> Result<Vec<String>, String> {
    persist_dispositions(
        project,
        actions,
        expected_evidence_hashes,
        expected_planning_revision,
        &std::collections::HashMap::new(),
    )
}

fn is_plan_review_action(action: &ManagerActionV1) -> bool {
    DispositionWork::from_action(&action.kind).is_some()
        && !action
            .evidence
            .iter()
            .any(|id| id.starts_with("code-review-finding:"))
}

/// Select one persisted repair and checkpoint its active state before starting
/// Planner. Task chat and status then recover through the normal planning-work path.
pub fn next_plan_repair(project: &mut Project) -> Option<(String, String, String)> {
    if project.active_turn.is_some() || project.activity.manager.is_some() {
        return None;
    }
    let index = project.planning_work.iter().position(|work| {
        work.key.starts_with("task:manager-plan-repair:") && work.status == WorkStatus::Todo
    })?;
    let feature_id = project.planning_work[index].feature_id.clone()?;
    let original = project.planning_work.clone();
    project.planning_work[index].status = WorkStatus::InProgress;
    project.planning_work[index].detail.push_str(
        "\n\nThe Manager owns this repair and is directing a scoped Planner pass against the cited finding. Any changed candidate will require fresh user confirmation, independent review, and Manager verification.",
    );
    let key = project.planning_work[index].key.clone();
    let request = project.planning_work[index].request.clone();
    if project.save_planning_work().is_err() {
        project.planning_work = original;
        return None;
    }
    Some((key, request, feature_id))
}

pub(crate) fn has_pending_plan_repair(project: &Project, feature_id: &str) -> bool {
    project.planning_work.iter().any(|work| {
        work.key.starts_with("task:manager-plan-repair:")
            && work.feature_id.as_deref() == Some(feature_id)
            && work.status != WorkStatus::Done
    })
}

pub(crate) fn has_pending_plan_contest(project: &Project, feature_id: &str) -> bool {
    project.planning_work.iter().any(|work| {
        work.key.starts_with("task:manager-plan-contest:")
            && work.feature_id.as_deref() == Some(feature_id)
            && work.status != WorkStatus::Done
    })
}

pub(crate) fn next_plan_arbitration(
    project: &mut Project,
) -> Option<(String, crate::app::root::plan_review::ArbitrationContext)> {
    if project.active_turn.is_some()
        || project.activity.manager.is_some()
        || project.activity.plan_review.is_some()
    {
        return None;
    }
    let index = project.planning_work.iter().position(|work| {
        work.key.starts_with("task:manager-plan-contest:")
            && work.status == WorkStatus::NeedsAttention
            && work
                .detail
                .lines()
                .filter(|line| {
                    line.starts_with("Arbitration attempt failed: ")
                        || line.starts_with("Independent arbitration receipt: ")
                })
                .count()
                < super::MAX_PLAN_ARBITRATION_ATTEMPTS
    })?;
    let original = project.planning_work.clone();
    let work = &project.planning_work[index];
    let value = |name: &str| {
        work.detail
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let decode = |value: String| fields::decode(&value);
    let selection = (
        work.feature_id.clone()?,
        crate::app::root::plan_review::ArbitrationContext {
            finding_id: work.key.rsplit(':').next()?.to_owned(),
            evidence_id: value("Arbitration evidence: ")?,
            evidence_sha256: value("Arbitration evidence SHA-256: ")?,
            source_quote: decode(value("Arbitration source quote: ")?),
            source_content: String::new(),
            rationale: decode(value("Arbitration rationale: ")?),
        },
    );
    project.planning_work[index].status = WorkStatus::InProgress;
    if project.save_planning_work().is_err() {
        project.planning_work = original;
        return None;
    }
    Some(selection)
}

#[cfg(test)]
#[path = "dispositions/tests.rs"]
mod tests;
