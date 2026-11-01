use crate::core::{planning_work, state::PlannerState, validation::NormalizedTurn};

const STALE_LINK: &str = "Planning task feature identity mismatch:";

#[derive(Clone)]
struct TaskTarget {
    key: String,
    feature_id: Option<String>,
    feature_uid: Option<String>,
}

fn task_target(state: &PlannerState, key: &str) -> Option<TaskTarget> {
    let work = planning_work::find_card(state, key);
    if let Some(work) = work {
        return Some(TaskTarget {
            key: work.key,
            feature_id: work.feature_id,
            feature_uid: work.feature_uid,
        });
    }

    let synthetic = crate::core::ownership::synthesize_for_state(state);
    state
        .items
        .iter()
        .chain(&state.resolved_items)
        .chain(&synthetic)
        .find(|item| item.conversation_key() == key)
        .map(|item| TaskTarget {
            key: item.conversation_key().to_owned(),
            feature_id: item.feature_id.clone(),
            feature_uid: item.feature_uid.clone(),
        })
}

pub(super) fn task_feature_uid(state: &PlannerState, task: &str) -> Option<String> {
    task_target(state, task).and_then(|target| target.feature_uid)
}

pub(super) fn validate_task_link(
    state: &PlannerState,
    task: Option<&str>,
    expected_feature_uid: Option<&str>,
) -> Result<(), String> {
    let Some(task) = task else {
        return Ok(());
    };
    let target = task_target(state, task);
    let expected_uid = expected_feature_uid.map(str::to_owned).or_else(|| {
        target
            .as_ref()
            .and_then(|target| target.feature_uid.clone())
    });
    let Some(expected_uid) = expected_uid else {
        return Ok(());
    };
    let Some(target) = target else {
        return Err(format!(
            "{STALE_LINK} conversation {task} no longer resolves to its recorded feature UID {expected_uid}. No planning changes were applied; reconcile this link internally before retrying."
        ));
    };
    if target.feature_uid.as_deref() != Some(expected_uid.as_str()) {
        return Err(format!(
            "{STALE_LINK} conversation {task} was pinned to feature UID {expected_uid}, but its current task link resolves to UID {}. No planning changes were applied; reconcile this link internally before retrying.",
            target.feature_uid.as_deref().unwrap_or("no immutable UID")
        ));
    }
    if state.active_features.iter().any(|(_, markdown)| {
        crate::domain::ArtifactIdentity::from_markdown(markdown)
            .ok()
            .flatten()
            .is_some_and(|identity| identity.uid == expected_uid)
    }) {
        return Ok(());
    }
    let replacement = target.feature_id.as_deref().and_then(|feature_id| {
        state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
    });
    let detail = replacement
        .map(|(id, _)| format!("display ID {id} now belongs to a different feature"))
        .unwrap_or_else(|| "the linked feature is no longer active".into());
    Err(format!(
        "{STALE_LINK} conversation {} references UID {expected_uid}, but {detail}. No planning changes were applied; reconcile this link internally before retrying.",
        target.key
    ))
}

pub(super) fn validate_updates(
    state: &PlannerState,
    task: Option<&str>,
    expected_feature_uid: Option<&str>,
    normalized: &NormalizedTurn,
) -> Result<(), String> {
    let Some(task) = task else {
        return Ok(());
    };
    let target = task_target(state, task);
    for feature_id in normalized
        .document_updates
        .iter()
        .filter_map(|(document_id, _)| document_id.strip_prefix("feature:"))
    {
        match expected_feature_uid.or_else(|| {
            target
                .as_ref()
                .and_then(|target| target.feature_uid.as_deref())
        }) {
            Some(expected_uid) => {
                let target_identity = state
                    .active_features
                    .iter()
                    .find(|(id, _)| id == feature_id)
                    .and_then(|(_, markdown)| {
                        crate::domain::ArtifactIdentity::from_markdown(markdown)
                            .ok()
                            .flatten()
                    });
                if target_identity.is_none_or(|identity| identity.uid != expected_uid) {
                    return Err(format!(
                        "{STALE_LINK} conversation {task} cannot update feature {feature_id}; the target does not have its recorded UID {expected_uid}. No planning changes were applied.",
                    ));
                }
            }
            None => {
                if let Some(existing) = state
                    .active_features
                    .iter()
                    .find(|(id, _)| id == feature_id)
                {
                    let detail = target
                        .as_ref()
                        .and_then(|target| target.feature_id.as_deref())
                        .map(|id| format!("legacy feature ID {id} has no immutable UID"))
                        .unwrap_or_else(|| "the conversation has no immutable feature link".into());
                    return Err(format!(
                        "{STALE_LINK} conversation {task} cannot update existing feature {}; {detail}. No planning changes were applied; reconcile this link internally before retrying.",
                        existing.0
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "feature_target/tests.rs"]
mod tests;
