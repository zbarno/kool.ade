//! Creation and refresh of the canonical plan-confirmation card record.
//! Every factor that shapes the decision arrives flattened but explicit,
//! which is why the constructors take many arguments.

use super::details::detail;
use super::reduce::RootAnchor;
use super::*;

#[allow(clippy::too_many_arguments)] // Flattened event carries every factor shaping the decision.
pub(super) fn create_record(
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
pub(super) fn update_record(
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
pub(super) fn new_payload(
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

pub(super) fn confirmation_key(feature_uid: &str, cycle_uid: Option<&str>) -> String {
    let tail: String = feature_uid.replace('-', "").chars().take(12).collect();
    match cycle_uid {
        Some(cycle) => {
            let segment: String = cycle.replace('-', "").chars().take(8).collect();
            format!("plan-confirmation-{tail}-{segment}")
        }
        None => format!("plan-confirmation-{tail}"),
    }
}

pub(super) fn should_notify(
    previous: Option<bool>,
    ready: bool,
    drift: bool,
    confirmed: bool,
) -> bool {
    (drift && confirmed) || previous.is_none() || previous == Some(!ready)
}

#[allow(clippy::too_many_arguments)] // Every identity is needed to route copy correctly.
pub(super) fn push_announcement(
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

pub(super) fn title(feature_title: &str) -> String {
    format!("Review and approve plan: {feature_title}")
}
