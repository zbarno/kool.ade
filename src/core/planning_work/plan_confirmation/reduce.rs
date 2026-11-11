//! Reduce step of the canonical plan-confirmation card: recomputes each
//! feature's card from authoritative lifecycle evidence only.

use super::*;
use crate::core::lifecycle::plan_snapshot;
use sha2::{Digest, Sha256};

const UNPARSEABLE_REVISION: &str = "unreadable";

pub(super) fn reconcile_feature(
    state: &PlannerState,
    feature_id: &str,
    body: &str,
    work: &mut Vec<Work>,
    report: &mut PlanConfirmationReport,
    excluded_keys: &BTreeSet<String>,
) -> anyhow::Result<()> {
    let Some(meta) = crate::domain::ChangeMetadata::from_markdown(body)
        .ok()
        .flatten()
    else {
        return Ok(()); // Not a durable change yet; nothing to anchor.
    };
    if !matches!(
        meta.status,
        crate::domain::ChangeStatus::Draft | crate::domain::ChangeStatus::Ready
    ) {
        return Ok(()); // Implementing/Implemented/Abandoned: gate is past.
    }
    let Some(anchor) = find_intent_root(work, &meta.uid) else {
        return Ok(()); // Legacy feature without a TaskIntent: old UI path.
    };
    let feature_title = body
        .lines()
        .next()
        .unwrap_or(feature_id)
        .trim_start_matches('#')
        .trim()
        .to_owned();
    let published = meta.status == crate::domain::ChangeStatus::Ready;

    let (snapshot, ready, cause, blockers) = match build_snapshot(state, feature_id, body) {
        Some(snapshot) => {
            match plan_snapshot::current_readiness(&state.planning_store, &snapshot) {
                Ok(()) => (Some(snapshot), true, None, Vec::new()),
                Err(message) => (
                    Some(snapshot),
                    false,
                    Some(classify_not_ready(&message)),
                    vec![message],
                ),
            }
        }
        None => (
            None,
            false,
            Some(PlanNotReadyCause::MalformedDocument),
            vec![
                "The plan document could not be parsed into a reviewable plan. \
                     Ask in this conversation to repair the plan document."
                    .into(),
            ],
        ),
    };
    let hash = snapshot
        .as_ref()
        .map(|snapshot| snapshot.content_hash.clone())
        .unwrap_or_else(|| sha256_of(body));
    let spec_revision = snapshot
        .as_ref()
        .map(|snapshot| snapshot.spec_revision.clone())
        .unwrap_or_else(|| UNPARSEABLE_REVISION.to_owned());

    let confirmed = snapshot
        .as_ref()
        .map(|snapshot| {
            plan_snapshot::has_current_user_confirmation(&state.planning_store, snapshot)
                .unwrap_or(false)
        })
        .unwrap_or(false);
    let review_failed = snapshot
        .as_ref()
        .and_then(|snapshot| {
            plan_snapshot::plan_review_verdict(&state.planning_store, snapshot)
                .ok()
                .flatten()
        })
        .is_some_and(|verdict| !verdict.pass);
    let approved =
        crate::core::workflow::feature_approved(&state.planning_store, &state.workflow, feature_id);

    // Keeper preference is a total order: current-hash match, then freshest
    // evaluation, then smallest key (see `beats_candidate`).
    let candidates: Vec<usize> = work
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.kind == WorkKind::PlanConfirmation
                && item.feature_uid.as_deref() == Some(meta.uid.as_str())
        })
        .map(|(index, _)| index)
        .collect();
    let mut keeper_choice: Option<usize> = None;
    for cand in &candidates {
        if let Some(current) = keeper_choice {
            if beats_candidate(work, *cand, current, &hash) {
                keeper_choice = Some(*cand);
            }
        } else {
            keeper_choice = Some(*cand);
        }
    }
    let Some(keeper) = keeper_choice else {
        return records::create_record(
            work,
            &anchor,
            &meta,
            feature_id,
            &feature_title,
            published,
            ready,
            cause,
            blockers,
            &hash,
            &spec_revision,
            report,
        );
    };
    let mut extras: Vec<usize> = candidates
        .into_iter()
        .filter(|index| Some(*index) != keeper_choice)
        .collect();
    extras.sort_unstable();
    extras.reverse();
    for extra in extras {
        work.remove(extra);
        report.changed = true;
        report.duplicates_removed += 1;
    }
    if excluded_keys.contains(work[keeper].key.as_str()) {
        return Ok(()); // Owned by a concurrent write; revisit next pass.
    }
    records::update_record(
        &mut work[keeper],
        &anchor,
        &meta,
        feature_id,
        &feature_title,
        published,
        ready,
        cause,
        blockers,
        confirmed,
        review_failed,
        approved,
        &hash,
        &spec_revision,
        report,
    )
}

fn build_snapshot(
    state: &PlannerState,
    feature_id: &str,
    body: &str,
) -> Option<crate::core::lifecycle::PlanSnapshotV1> {
    let identity = crate::domain::ArtifactIdentity::from_markdown(body)
        .ok()
        .flatten()?;
    let contract = crate::core::workflow::feature_contract(body);
    plan_snapshot::from_feature_with_author(
        &state.planning_store,
        feature_id,
        &identity,
        body,
        &contract,
    )
    .ok()
}

fn classify_not_ready(message: &str) -> PlanNotReadyCause {
    if message.contains("not marked ready") || message.contains("evidence no longer passes") {
        PlanNotReadyCause::RequirementsOutstanding
    } else {
        PlanNotReadyCause::AssessmentNeeded
    }
}

/// Prompt the manager to run a readiness evaluation once per spec revision,
/// for every card whose blocker class needs an evaluation turn. Markers live
/// in the payloads written this pass, so repeats with identical evidence
/// cannot double-prompt.
pub(super) fn manage_evaluation_prompts(work: &mut [Work], report: &mut PlanConfirmationReport) {
    for item in work.iter_mut() {
        let Some(payload) = item.plan_confirmation.as_mut() else {
            continue;
        };
        let promotable =
            !payload.ready && payload.cause != Some(PlanNotReadyCause::MalformedDocument);
        if promotable
            && payload.evaluation_prompted_for.as_deref()
                != Some(payload.snapshot_spec_revision.as_str())
        {
            payload.evaluation_prompted_for = Some(payload.snapshot_spec_revision.clone());
            report.evaluations_wanted.push(ReadinessEvaluationRequest {
                work_key: item.key.clone(),
                feature_id: item.feature_id.clone().unwrap_or_default(),
                feature_title: item.title.clone(),
            });
        } else if !promotable {
            payload.evaluation_prompted_for = None;
        }
    }
}

/// Total-order preference between duplicate canonical cards: current-hash
/// match first, then freshest evaluation, then lexically-smallest key as the
/// deterministic tiebreak. Returns true when `candidate` outranks `incumbent`.
fn beats_candidate(work: &[Work], candidate: usize, incumbent: usize, hash: &str) -> bool {
    let hash_match = |index: usize| {
        work[index]
            .plan_confirmation
            .as_ref()
            .is_some_and(|payload| payload.snapshot_content_hash == hash)
    };
    let (cand, incum) = (hash_match(candidate), hash_match(incumbent));
    if cand != incum {
        return cand;
    }
    let timestamp = |index: usize| {
        work[index]
            .plan_confirmation
            .as_ref()
            .map(|payload| payload.evaluated_at_unix_ms)
            .unwrap_or(0)
    };
    if timestamp(candidate) != timestamp(incumbent) {
        return timestamp(candidate) > timestamp(incumbent);
    }
    work[candidate].key.as_str() < work[incumbent].key.as_str()
}

/// Owned linkage facts copied from the TaskIntent root so that later vector
/// mutations (duplicate removal, field updates) cannot alias the root row.
#[derive(Debug, Clone)]
pub(super) struct RootAnchor {
    pub(super) uid: String,
    pub(super) task_intent_uid: String,
    pub(super) cycle: Option<String>,
}

fn find_intent_root(work: &[Work], feature_uid: &str) -> Option<RootAnchor> {
    let mut roots = work.iter().filter(|item| {
        item.parent_uid.is_none()
            && item.kind.starts_task_intent()
            && item.feature_uid.as_deref() == Some(feature_uid)
    });
    let root = roots.next()?;
    (roots.next().is_none()).then(|| RootAnchor {
        uid: root.uid.clone(),
        task_intent_uid: root
            .task_intent_root_uid()
            .map(str::to_owned)
            .unwrap_or_else(|| feature_uid.to_owned()),
        cycle: root.task_intent_cycle_uid.clone(),
    })
}

fn sha256_of(input: &str) -> String {
    format!("{:x}", Sha256::digest(input.as_bytes()))
}
