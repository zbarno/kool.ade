use crate::{
    app::session::Project,
    core::planning_work::{Work, WorkKind, WorkStatus},
    harness::prompt_catalog::ManagerActionV1,
};
#[path = "dispositions/fields.rs"]
mod fields;
#[path = "dispositions/kind.rs"]
mod kind;
use kind::DispositionWork;
use sha2::Digest;

/// Persist validated Manager decisions as ordinary planning work, using a
/// stable candidate/finding identity so repeated Manager runs cannot duplicate cards.
pub fn persist_plan_dispositions(
    project: &mut Project,
    actions: &[ManagerActionV1],
    expected_evidence_hashes: &std::collections::HashMap<String, String>,
    expected_planning_revision: &str,
) -> Result<Vec<String>, String> {
    if !actions
        .iter()
        .any(|action| DispositionWork::from_action(&action.kind).is_some())
    {
        return Ok(Vec::new());
    }
    let current_revision = project
        .state
        .planning_store
        .revision()
        .map_err(|error| error.to_string())?;
    if current_revision != expected_planning_revision
        || project.state.baseline_planning_revision != expected_planning_revision
    {
        return Ok(vec![
            "Skipped stale Manager dispositions because planning state changed during the Manager run; refresh and review again.".into(),
        ]);
    }
    let reviews = super::catalog::reviews::current_review_evidence(project);
    let current_evidence_hashes = reviews
        .iter()
        .flat_map(|review| std::iter::once(&review.item).chain(review.supporting_items.iter()))
        .map(|item| {
            (
                item.id.clone(),
                format!("{:x}", sha2::Sha256::digest(item.content.as_bytes())),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let original = project.planning_work.clone();
    let mut messages = Vec::new();
    let mut changed = false;
    for action in actions {
        let Some(kind) = DispositionWork::from_action(&action.kind) else {
            continue;
        };
        if action
            .evidence
            .iter()
            .any(|id| expected_evidence_hashes.get(id) != current_evidence_hashes.get(id))
        {
            messages.push(format!(
                "Skipped stale Manager disposition for {}; its cited evidence changed.",
                action.target
            ));
            continue;
        }
        let Some((feature_id, finding_id)) = action.target.split_once('#') else {
            messages.push("Skipped a plan-review disposition with an invalid target.".into());
            continue;
        };
        let Some(review) = reviews.iter().find(|review| {
            action
                .evidence
                .iter()
                .any(|evidence| evidence == &review.id)
                && review.target.feature_id == feature_id
                && review.target.revision == action.expected_revision
                && review.target.finding_ids.iter().any(|id| id == finding_id)
        }) else {
            messages.push(format!(
                "Skipped stale Manager disposition for {feature_id}#{finding_id}; the plan review changed."
            ));
            continue;
        };
        let Some((_, markdown)) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
        else {
            continue;
        };
        let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .ok()
            .flatten()
        else {
            continue;
        };
        let key = format!(
            "task:manager-plan-{}:{}:{}",
            kind.key(),
            action.expected_revision,
            finding_id
        );
        if project.planning_work.iter().any(|work| work.key == key) {
            continue;
        }
        let verdict: crate::harness::prompt_catalog::ReviewVerdictV1 =
            serde_json::from_str(&review.item.content).map_err(|error| error.to_string())?;
        let finding = verdict
            .findings
            .iter()
            .find(|finding| finding.id == finding_id)
            .ok_or_else(|| format!("validated finding {finding_id} disappeared"))?;
        let reason = crate::app::root::plan_review::verdict::sanitize_text(&action.reason, 2048);
        let title = format!("{}: {}", kind.title(), finding_id);
        let request = format!(
            "{} for {feature_id} based on independent plan review finding {finding_id}. {reason}",
            kind.request()
        );
        let arbitration_detail = if let Some(claim) = action.contradiction.as_ref() {
            let source_quote = fields::encode(&claim.source_quote)?;
            let rationale = fields::encode(&claim.explanation)?;
            let source_hash = expected_evidence_hashes
                .get(&claim.source_evidence_id)
                .map(String::as_str)
                .unwrap_or("missing");
            format!(
                "\nArbitration finding claim: {}\nArbitration evidence: {}\nArbitration evidence SHA-256: {source_hash}\nArbitration source quote: {source_quote}\nArbitration rationale: {rationale}",
                crate::app::root::plan_review::verdict::sanitize_text(&claim.finding_claim, 1024),
                claim.source_evidence_id,
            )
        } else {
            String::new()
        };
        let detail = format!(
            "Manager disposition: {}\nSnapshot hash: {}\nReview evidence: {}\nRequirement: {}\nFinding: {}\nLocation: {}\nCounterexample: {}\nManager rationale: {reason}\nEvidence IDs: {}{}",
            kind.label(),
            action.expected_revision,
            review.id,
            finding.requirement,
            finding.proposed_correction,
            finding.location,
            finding.repro.as_deref().unwrap_or("not provided"),
            action.evidence.join(", "),
            arbitration_detail,
        );
        let mut work = Work::new(String::new(), title, request, detail);
        work.kind = WorkKind::PlanReviewDisposition;
        work.status = kind.status();
        work.feature_id = Some(feature_id.to_owned());
        work.feature_uid = Some(identity.uid);
        work.key = key;
        project.planning_work.push(work);
        changed = true;
    }
    if changed && let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        return Err(error);
    }
    Ok(messages)
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
