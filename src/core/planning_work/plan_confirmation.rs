//! Canonical state-bound "Review and approve plan" action task.
//!
//! At most one durable review card is maintained for every active feature
//! attached to a TaskIntent. The card presents the exact immutable plan
//! snapshot the user may approve. Its status derives only from typed
//! lifecycle evidence (persisted readiness, user-confirmation receipts,
//! review verdicts, workflow approval); chat text and card counts never
//! drive it, so the board never infers approval or readiness on its own.

use crate::core::lifecycle::plan_snapshot;
use crate::core::state::PlannerState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use super::storage::now_ms;
use super::{Work, WorkKind, WorkStatus};
use details::detail;

mod details;
#[cfg(test)]
mod tests;

pub const PLAN_CONFIRMATION_SCHEMA: u16 = 1;

const UNPARSEABLE_REVISION: &str = "unreadable";

/// Why a plan is not approvable yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanNotReadyCause {
    /// No valid persisted readiness assessment exists for the current
    /// specification revision (or it lags newer clarification records).
    AssessmentNeeded,
    /// A persisted assessment exists but lists outstanding work: blocking
    /// questions, unassessed axes, or untraced requirements.
    RequirementsOutstanding,
    /// The feature document could not be parsed into a plan snapshot.
    MalformedDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanConfirmationV1 {
    pub schema_version: u16,
    pub feature_uid: String,
    /// SHA-256 of the immutable plan snapshot presented for approval.
    pub snapshot_content_hash: String,
    pub snapshot_spec_revision: String,
    /// True when the change is a published product document, false while it
    /// is still an editable draft.
    pub pin_is_published: bool,
    pub ready: bool,
    pub cause: Option<PlanNotReadyCause>,
    pub blockers: Vec<String>,
    /// Specification revision for which a readiness-evaluation turn was last
    /// requested; prevents hot re-prompt loops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_prompted_for: Option<String>,
    /// The readiness state the user was last told about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announced_ready: Option<bool>,
    pub evaluated_at_unix_ms: u64,
}

impl PlanConfirmationV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == PLAN_CONFIRMATION_SCHEMA,
            "unsupported plan confirmation schema version {}",
            self.schema_version
        );
        anyhow::ensure!(
            is_sha256(&self.snapshot_content_hash),
            "plan confirmation references an unparsed plan snapshot"
        );
        anyhow::ensure!(
            uuid::Uuid::parse_str(&self.feature_uid).is_ok(),
            "plan confirmation references an unknown feature identity"
        );
        if self.ready {
            anyhow::ensure!(
                self.cause.is_none() && self.blockers.is_empty(),
                "ready plan confirmation still names blockers"
            );
        } else {
            anyhow::ensure!(
                self.cause.is_some() && !self.blockers.is_empty(),
                "blocked plan confirmation must name why it is blocked"
            );
        }
        Ok(())
    }
}

/// Transition worth telling the user about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanConfirmationAnnouncement {
    pub work_uid: String,
    pub work_key: String,
    pub feature_id: String,
    pub feature_title: String,
    pub ready: bool,
    pub created_this_pass: bool,
    pub excerpt: String,
}

/// One readiness-evaluation turn the app layer should try to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessEvaluationRequest {
    pub work_key: String,
    pub feature_id: String,
    pub feature_title: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlanConfirmationReport {
    pub changed: bool,
    pub announcements: Vec<PlanConfirmationAnnouncement>,
    pub evaluations_wanted: Vec<ReadinessEvaluationRequest>,
    pub duplicates_removed: usize,
}

/// Recompute every canonical plan-confirmation card from authoritative
/// lifecycle evidence. Idempotent: a second pass with identical evidence is
/// a no-op. Keys in `excluded_keys` must not be mutated (they belong to a
/// concurrent write).
pub fn reconcile(
    state: &PlannerState,
    work: &mut Vec<Work>,
    excluded_keys: &BTreeSet<String>,
) -> anyhow::Result<PlanConfirmationReport> {
    let mut report = PlanConfirmationReport::default();
    for (feature_id, body) in state.active_features.iter() {
        reconcile_feature(state, feature_id, body, work, &mut report, excluded_keys)?;
    }
    manage_evaluation_prompts(work.as_mut_slice(), &mut report);
    Ok(report)
}

fn reconcile_feature(
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
        return create_record(
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
    update_record(
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

#[allow(clippy::too_many_arguments)] // Flattened event carries every factor shaping the decision.
fn create_record(
    work: &mut Vec<Work>,
    root: &RootAnchor,
    meta: &crate::domain::ChangeMetadata,
    feature_id: &str,
    feature_title: &str,
    published: bool,
    ready: bool,
    cause: Option<PlanNotReadyCause>,
    blockers: Vec<String>,
    hash: &str,
    spec_revision: &str,
    report: &mut PlanConfirmationReport,
) -> anyhow::Result<()> {
    let card_key = confirmation_key(&meta.uid, root.cycle.as_deref());
    let mut card = Work::new(
        card_key.clone(),
        title(feature_title),
        format!(
            "Review and approve the immutable plan for “{feature_title}” before \
             implementation begins."
        ),
        detail(
            ready,
            feature_title,
            &blockers,
            hash,
            spec_revision,
            published,
            false,
            false,
            false,
            false,
        ),
    );
    card.status = if ready {
        WorkStatus::InReview
    } else {
        WorkStatus::NeedsAttention
    };
    card.kind = WorkKind::PlanConfirmation;
    card.feature_id = Some(feature_id.to_owned());
    card.feature_uid = Some(meta.uid.clone());
    card.parent_uid = Some(root.uid.clone());
    card.task_intent_uid = Some(root.task_intent_uid.clone());
    card.task_intent_cycle_uid = root.cycle.clone();
    card.plan_confirmation = Some(new_payload(
        &meta.uid,
        hash,
        spec_revision,
        published,
        ready,
        cause,
        blockers.clone(),
    ));
    let (card_uid, card_key) = (card.uid.clone(), card_key.clone());
    work.push(card);
    report.changed = true;
    push_announcement(
        &card_uid,
        &card_key,
        feature_id,
        feature_title,
        ready,
        true,
        &blockers,
        report,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Each lifecycle factor is individually significant.
fn update_record(
    card: &mut Work,
    root: &RootAnchor,
    meta: &crate::domain::ChangeMetadata,
    feature_id: &str,
    feature_title: &str,
    published: bool,
    ready: bool,
    cause: Option<PlanNotReadyCause>,
    blockers: Vec<String>,
    confirmed: bool,
    review_failed: bool,
    approved: bool,
    hash: &str,
    spec_revision: &str,
    report: &mut PlanConfirmationReport,
) -> anyhow::Result<()> {
    let previous = card
        .plan_confirmation
        .as_ref()
        .filter(|payload| payload.feature_uid == meta.uid)
        .cloned();
    let previous_announced = previous
        .as_ref()
        .and_then(|payload| payload.announced_ready);
    let drift = previous
        .as_ref()
        .map(|payload| payload.snapshot_content_hash != hash)
        .unwrap_or(false);

    let status = if !ready {
        WorkStatus::NeedsAttention
    } else if confirmed && drift {
        WorkStatus::InReview // Receipt bound to an older snapshot: re-approve.
    } else if confirmed {
        if approved {
            WorkStatus::Done
        } else if review_failed {
            WorkStatus::NeedsAttention
        } else {
            WorkStatus::InProgress // Independent review still running.
        }
    } else {
        WorkStatus::InReview
    };
    let detail_text = detail(
        ready,
        feature_title,
        &blockers,
        hash,
        spec_revision,
        published,
        confirmed,
        drift,
        review_failed,
        approved,
    );

    // Carry the previous payload forward so prompt/notification markers and
    // the evaluation timestamp survive no-op passes.
    let mut payload = previous
        .filter(|_| card.kind == WorkKind::PlanConfirmation)
        .unwrap_or_else(|| {
            new_payload(
                &meta.uid,
                hash,
                spec_revision,
                published,
                ready,
                cause,
                blockers.clone(),
            )
        });
    let unchanged = payload.snapshot_content_hash == hash
        && payload.snapshot_spec_revision == spec_revision
        && payload.pin_is_published == published
        && payload.ready == ready
        && payload.cause == cause
        && payload.blockers == blockers
        && card.status == status
        && card.detail == detail_text
        && card.title == title(feature_title)
        && card.parent_uid.as_deref() == Some(root.uid.as_str())
        && card.task_intent_uid.as_deref() == Some(root.task_intent_uid.as_str());
    if unchanged {
        return Ok(()); // Idempotent steady state; no record churn.
    }
    payload.snapshot_content_hash = hash.to_owned();
    payload.snapshot_spec_revision = spec_revision.to_owned();
    payload.pin_is_published = published;
    payload.ready = ready;
    payload.cause = cause;
    payload.blockers = blockers.clone();
    payload.evaluated_at_unix_ms = now_ms();

    card.status = status;
    card.kind = WorkKind::PlanConfirmation;
    card.title = title(feature_title);
    card.feature_id = Some(feature_id.to_owned());
    card.feature_uid = Some(meta.uid.clone());
    card.parent_uid = Some(root.uid.clone());
    card.task_intent_uid = Some(root.task_intent_uid.clone());
    card.task_intent_cycle_uid = root.cycle.clone();
    card.detail = detail_text;
    if should_notify(previous_announced, ready, drift, confirmed) {
        payload.announced_ready = Some(ready);
        card.plan_confirmation = Some(payload);
        push_announcement(
            &card.uid,
            &card.key,
            feature_id,
            feature_title,
            ready,
            false,
            &blockers,
            report,
        );
    } else {
        card.plan_confirmation = Some(payload);
    }
    report.changed = true;
    Ok(())
}

/// Fresh payload; the prompt pass stamps `evaluation_prompted_for` exactly
/// once per spec revision.
fn new_payload(
    feature_uid: &str,
    hash: &str,
    spec_revision: &str,
    published: bool,
    ready: bool,
    cause: Option<PlanNotReadyCause>,
    blockers: Vec<String>,
) -> PlanConfirmationV1 {
    PlanConfirmationV1 {
        schema_version: PLAN_CONFIRMATION_SCHEMA,
        feature_uid: feature_uid.to_owned(),
        snapshot_content_hash: hash.to_owned(),
        snapshot_spec_revision: spec_revision.to_owned(),
        pin_is_published: published,
        ready,
        cause,
        blockers,
        evaluation_prompted_for: None,
        announced_ready: None,
        evaluated_at_unix_ms: now_ms(),
    }
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
fn manage_evaluation_prompts(work: &mut [Work], report: &mut PlanConfirmationReport) {
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
struct RootAnchor {
    uid: String,
    task_intent_uid: String,
    cycle: Option<String>,
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

fn confirmation_key(feature_uid: &str, cycle_uid: Option<&str>) -> String {
    let tail: String = feature_uid.replace('-', "").chars().take(12).collect();
    match cycle_uid {
        Some(cycle) => {
            let segment: String = cycle.replace('-', "").chars().take(8).collect();
            format!("plan-confirmation-{tail}-{segment}")
        }
        None => format!("plan-confirmation-{tail}"),
    }
}

fn should_notify(previous: Option<bool>, ready: bool, drift: bool, confirmed: bool) -> bool {
    (drift && confirmed) || previous.is_none() || previous == Some(!ready)
}

#[allow(clippy::too_many_arguments)] // Every identity is needed to route copy correctly.
fn push_announcement(
    work_uid: &str,
    work_key: &str,
    feature_id: &str,
    feature_title: &str,
    ready: bool,
    created: bool,
    blockers: &[String],
    report: &mut PlanConfirmationReport,
) {
    let excerpt = if ready {
        "The plan is ready for your review and approval."
    } else if let Some(blocker) = blockers.first() {
        blocker.as_str()
    } else {
        "The plan is not ready for approval yet."
    }
    .to_owned();
    report.announcements.push(PlanConfirmationAnnouncement {
        work_uid: work_uid.to_owned(),
        work_key: work_key.to_owned(),
        feature_id: feature_id.to_owned(),
        feature_title: feature_title.to_owned(),
        ready,
        created_this_pass: created,
        excerpt,
    });
}

pub fn title(feature_title: &str) -> String {
    format!("Review and approve plan: {feature_title}")
}

fn sha256_of(input: &str) -> String {
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

fn is_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}
