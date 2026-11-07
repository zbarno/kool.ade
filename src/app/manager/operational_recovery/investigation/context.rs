use super::*;

/// One remaining investigation claim can resume after the exact missing source
/// feature becomes active. Repeating an unchanged failure cannot reset its budget.
pub(super) fn rearm(project: &mut Project, index: usize) -> bool {
    let task = &project.planning_work[index];
    let recovery = task.operational_recovery.as_ref().unwrap();
    let Some(item) = project.state.items.iter().find(|item| {
        item.uid.as_deref() == Some(&recovery.source_uid)
            && item.feature_uid == recovery.expected_feature_uid
            && item.status == crate::domain::ItemStatus::Open
            && item.authority == crate::domain::Authority::Agent
    }) else {
        return false;
    };
    let Some(feature_id) = item.feature_id.as_ref() else {
        return false;
    };
    if recovery.investigation_claims >= 2
        || !recovery
            .repair_error
            .as_ref()
            .is_some_and(|error| references_feature(error, feature_id))
        || [&task.uid, &recovery.source_uid]
            .into_iter()
            .chain(recovery.expected_feature_uid.iter())
            .any(|uid| {
                project
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::planning_id(uid))
            })
        || project
            .state
            .active_features
            .iter()
            .filter(|(id, text)| {
                id == feature_id
                    && crate::domain::ArtifactIdentity::from_markdown(text)
                        .ok()
                        .flatten()
                        .is_some_and(|identity| {
                            Some(&identity.uid) == recovery.expected_feature_uid.as_ref()
                        })
            })
            .count()
            != 1
    {
        return false;
    }
    let key = item.id.clone();
    let original = project.planning_work.clone();
    let task = &mut project.planning_work[index];
    let recovery = task.operational_recovery.as_mut().unwrap();
    recovery.repair_error = None;
    recovery.next_attempt_ms = chrono::Utc::now().timestamp_millis() + 30_000;
    task.status = WorkStatus::Todo;
    task.detail = "The original feature context was restored. Manager scheduled the remaining bounded investigation attempt after its cooldown.".into();
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        project.queue.last_error =
            format!("Restored investigation context could not be checkpointed: {error}");
    } else {
        project.investigation_attempted.remove(&key);
    }
    true
}

fn references_feature(error: &str, feature_id: &str) -> bool {
    error
        .split("invalid feature reference ")
        .skip(1)
        .any(|tail| {
            tail.split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                .next()
                == Some(feature_id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repaired_context_requires_the_exact_failed_feature_reference() {
        assert!(references_feature(
            "repair: invalid feature reference F1. Retry",
            "F1"
        ));
        assert!(!references_feature(
            "repair: invalid feature reference F10. Retry",
            "F1"
        ));
        assert!(!references_feature("Provider is unavailable", "F1"));
    }
}
