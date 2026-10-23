use crate::artifacts::planning_store::{PlanningStore, StoreError};
use crate::core::lifecycle::{FeatureEvent, FeatureGoal};

use super::{
    Snapshot,
    records::{
        AdditionalTransition, EventProjection, append_transition,
        append_transition_with_additional_checks, event_path, event_record, feature_actor,
        feature_path, now_unix_ms, stale,
    },
    task_transition,
};

#[cfg(test)]
#[path = "feature_event/tests.rs"]
mod tests;

pub(super) fn apply(
    store: &PlanningStore,
    feature_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    proposal: FeatureEvent,
) -> Result<FeatureGoal, StoreError> {
    let (path, snapshot) = read_for_apply(store, feature_uid, expected_revision)?;
    commit_after_read(
        store,
        feature_uid,
        &path,
        expected_revision,
        event_uid,
        proposal,
        snapshot,
    )
}

fn read_for_apply(
    store: &PlanningStore,
    feature_uid: &str,
    expected_revision: u64,
) -> Result<(String, Snapshot<FeatureGoal>), StoreError> {
    let path = feature_path(feature_uid);
    let (snapshot, actual) = store.read_record::<Snapshot<FeatureGoal>>(&path)?;
    if actual != expected_revision || snapshot.record.revision != expected_revision {
        return Err(stale(&path, expected_revision, actual));
    }
    Ok((path, snapshot))
}

fn commit_after_read(
    store: &PlanningStore,
    feature_uid: &str,
    path: &str,
    expected_revision: u64,
    event_uid: &str,
    proposal: FeatureEvent,
    mut snapshot: Snapshot<FeatureGoal>,
) -> Result<FeatureGoal, StoreError> {
    let answers_resolved = matches!(proposal, FeatureEvent::AnswersResolved);
    let question_fence = if answers_resolved {
        ensure_blocking_questions_resolved(store, feature_uid)?
    } else {
        AdditionalTransition {
            changes: Vec::new(),
            checks: Vec::new(),
        }
    };
    if matches!(proposal, FeatureEvent::TasksCompleted) {
        task_transition::ensure_feature_tasks_complete(
            store,
            feature_uid,
            &snapshot.record.task_uids,
        )?;
    }
    snapshot
        .record
        .apply(expected_revision, proposal.clone())
        .map_err(StoreError::MalformedState)?;
    snapshot.revision = snapshot.record.revision;
    snapshot.updated_at_ms = now_unix_ms();
    let event_path = event_path(event_uid)?;
    let event = event_record(
        event_uid,
        feature_uid,
        None,
        expected_revision,
        feature_actor(&proposal),
        super::LifecycleEventKind::Feature(proposal),
        EventProjection {
            feature_revision_after: Some(snapshot.revision),
            ..EventProjection::default()
        },
    )?;
    if answers_resolved {
        append_transition_with_additional_checks(
            store,
            path,
            &snapshot,
            expected_revision,
            event_path,
            event,
            question_fence,
        )?;
    } else {
        append_transition(store, path, &snapshot, expected_revision, event_path, event)?;
    }
    Ok(snapshot.record)
}

fn ensure_blocking_questions_resolved(
    store: &PlanningStore,
    feature_uid: &str,
) -> Result<AdditionalTransition, StoreError> {
    use crate::core::planning::{QuestionPriority, QuestionStatus, QuestionStore};

    let questions = QuestionStore::new(store);
    let catalog_check = questions.feature_revision_check(feature_uid)?;
    let feature_questions = questions
        .list()?
        .into_iter()
        .filter(|question| {
            question.feature_uid == feature_uid && question.priority == QuestionPriority::Blocking
        })
        .collect::<Vec<_>>();
    let unresolved = feature_questions
        .iter()
        .filter(|question| {
            matches!(
                question.status,
                QuestionStatus::Draft | QuestionStatus::Open | QuestionStatus::Declined
            ) && question.deferred.is_none()
        })
        .map(|question| question.uid.as_str())
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        Ok(AdditionalTransition {
            changes: Vec::new(),
            checks: std::iter::once(catalog_check)
                .chain(
                    feature_questions
                        .iter()
                        .map(|question| questions.revision_check(&question.uid))
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .collect(),
        })
    } else {
        Err(StoreError::MalformedState(format!(
            "cannot resolve feature answers while blocking planning questions remain: {}",
            unresolved.join(", ")
        )))
    }
}
