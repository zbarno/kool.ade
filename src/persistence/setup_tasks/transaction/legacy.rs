//! Narrow compatibility for retired source aliases and pre-smoke rootless proof.
use crate::persistence::setup_tasks::{
    AttemptState, DependencyResolution, SetupTaskStateV1 as State, SetupTaskStoreV1,
};
use std::collections::BTreeSet;

pub(super) fn decode(bytes: &[u8]) -> anyhow::Result<SetupTaskStoreV1> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    let declared_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("unsupported setup store schema"))?;
    anyhow::ensure!(
        declared_version == 2 || declared_version == 3,
        "unsupported setup store schema"
    );
    let mut legacy_rootless = false;
    if let Some(tasks) = value
        .get_mut("tasks")
        .and_then(serde_json::Value::as_object_mut)
    {
        for (id, task) in tasks {
            if let Some(legacy) = task
                .as_object_mut()
                .and_then(|task| task.remove("legacySourceId"))
            {
                anyhow::ensure!(
                    legacy.is_null(),
                    "nonempty legacy setup source requires explicit migration"
                );
                legacy_rootless |= id == "setup:docker:rootless";
            }
        }
    }
    let mut store: SetupTaskStoreV1 = serde_json::from_value(value)?;
    // Schema 3 adds the additive `setupFlow` field; prior files upgrade in place.
    if store.schema_version == 2 {
        store.schema_version = 3;
    }
    let old_probe = legacy_rootless
        && store
            .tasks
            .get("setup:docker:rootless")
            .is_some_and(|task| {
                task.state == State::Verified
                    && task
                        .verified_result
                        .as_ref()
                        .is_some_and(|proof| proof.evidence_domain == "docker")
            });
    if old_probe {
        anyhow::ensure!(
            store.tasks.iter().all(|(id, task)| id == &task.id),
            "invalid saved setup task ID"
        );
        let mut invalid = BTreeSet::from(["setup:docker:rootless".to_owned()]);
        for id in super::super::graph::ordered_task_ids(&store)? {
            let task = store.tasks.get_mut(&id).expect("ordered task exists");
            let mut blocked = false;
            for dependency in &mut task.dependencies {
                if invalid.contains(&dependency.prerequisite_id) {
                    dependency.resolution = DependencyResolution::Pending;
                    blocked = true;
                }
            }
            if invalid.contains(&id) || blocked {
                anyhow::ensure!(
                    !task.attempts.iter().any(|attempt| matches!(
                        attempt.state,
                        AttemptState::NeedsApproval
                            | AttemptState::InProgress
                            | AttemptState::AwaitingVerification
                    )),
                    "legacy readiness migration requires its active setup operation to settle"
                );
                task.state = if blocked {
                    State::BlockedByPrerequisite
                } else {
                    State::NeedsVerification
                };
                task.verified_result = None;
                // Old broad Docker evidence is history, never current smoke proof.
                for attempt in &mut task.attempts {
                    if id == "setup:docker:rootless"
                        && attempt.state == AttemptState::Verified
                        && attempt
                            .verified_result
                            .as_ref()
                            .is_some_and(|proof| proof.evidence_domain == "docker")
                    {
                        attempt.state = AttemptState::Failed;
                        attempt.failure = Some(
                            crate::persistence::setup_tasks::FailureCode::SupersededByReadiness,
                        );
                        attempt.verified_result = None;
                        attempt.authorization_binding = None;
                        attempt.owner_instance_id = None;
                    }
                }
                task.blocked_by = task
                    .dependencies
                    .iter()
                    .filter(|dependency| dependency.resolution == DependencyResolution::Pending)
                    .map(|dependency| dependency.prerequisite_id.clone())
                    .collect();
                invalid.insert(id);
            }
        }
    }
    store.validate()?;
    Ok(store)
}

#[cfg(test)]
mod tests;
