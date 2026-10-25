use std::collections::{BTreeMap, BTreeSet};

use crate::app::setup_attention::catalog::MachineSnapshot;

use super::{
    AttemptState, DependencyResolution, FailureCode, SetupTaskStateV1, SetupTaskStoreV1,
    model::record,
};

/// Stable topological order. Unobserved prerequisites remain graph leaves and
/// block their child until a fresh snapshot supplies positive evidence.
pub fn ordered_task_ids(store: &SetupTaskStoreV1) -> anyhow::Result<Vec<String>> {
    let mut indegree = store
        .tasks
        .keys()
        .map(|id| (id.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeMap::<String, BTreeSet<String>>::new();
    for task in store.tasks.values() {
        for dependency in &task.dependencies {
            if store.tasks.contains_key(&dependency.prerequisite_id) {
                *indegree.get_mut(&task.id).expect("task ID was collected") += 1;
                children
                    .entry(dependency.prerequisite_id.clone())
                    .or_default()
                    .insert(task.id.clone());
            }
        }
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(id.clone()))
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(store.tasks.len());
    while let Some(id) = ready.pop_first() {
        ordered.push(id.clone());
        if let Some(dependents) = children.get(&id) {
            for child in dependents {
                let degree = indegree.get_mut(child).expect("child task exists");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(child.clone());
                }
            }
        }
    }
    anyhow::ensure!(
        ordered.len() == store.tasks.len(),
        "setup task dependency graph contains a cycle"
    );
    Ok(ordered)
}

pub(super) fn refresh_dependencies(
    store: &mut SetupTaskStoreV1,
    snapshot: &MachineSnapshot,
    generation: u64,
) -> anyhow::Result<bool> {
    let mut changed = false;
    for id in ordered_task_ids(store)? {
        let task = store.tasks.get(&id).cloned().expect("ordered task exists");
        let mut dependencies = task.dependencies.clone();
        let mut blocked_by = Vec::new();
        for dependency in &mut dependencies {
            dependency.resolution =
                resolve_dependency(&dependency.prerequisite_id, store, snapshot, generation);
            if dependency.resolution == DependencyResolution::Pending {
                blocked_by.push(dependency.prerequisite_id.clone());
            }
        }

        let has_blockers = !blocked_by.is_empty();
        let mut next_state = task.state;
        let mut attempts = task.attempts.clone();
        if has_blockers
            && !matches!(
                task.state,
                SetupTaskStateV1::Unsupported | SetupTaskStateV1::Superseded
            )
        {
            next_state = SetupTaskStateV1::BlockedByPrerequisite;
            stop_active_attempts(&mut attempts, snapshot.checked_at_unix_ms);
        } else if !has_blockers {
            match task.state {
                SetupTaskStateV1::Detected | SetupTaskStateV1::BlockedByPrerequisite => {
                    next_state = SetupTaskStateV1::ReadyToFix;
                }
                _ => {}
            }
        }

        let changed_task = dependencies != task.dependencies
            || blocked_by != task.blocked_by
            || next_state != task.state
            || attempts != task.attempts;
        if changed_task {
            let updated = store.tasks.get_mut(&id).expect("ordered task exists");
            updated.dependencies = dependencies;
            updated.blocked_by = blocked_by;
            updated.state = next_state;
            updated.attempts = attempts;
            if next_state == SetupTaskStateV1::BlockedByPrerequisite {
                updated.verified_result = None;
            }
            record(updated, snapshot.checked_at_unix_ms, generation);
            changed = true;
        }
    }
    Ok(changed)
}

fn resolve_dependency(
    id: &str,
    store: &SetupTaskStoreV1,
    snapshot: &MachineSnapshot,
    generation: u64,
) -> DependencyResolution {
    if let Some(parent) = store.tasks.get(id) {
        return parent
            .verified_result
            .as_ref()
            .filter(|_| parent.state == SetupTaskStateV1::Verified)
            .filter(|receipt| receipt.scan_generation == generation)
            .map(|receipt| DependencyResolution::SatisfiedByTask {
                scan_generation: receipt.scan_generation,
            })
            .unwrap_or(DependencyResolution::Pending);
    }
    if !snapshot.findings.iter().any(|finding| finding.id == id)
        && let Some(evidence_domain) = super::model::positive_domain_for(id, snapshot)
    {
        return DependencyResolution::SatisfiedByProbe {
            evidence_domain,
            scan_generation: generation,
            verified_at_unix_ms: snapshot.checked_at_unix_ms,
        };
    }
    DependencyResolution::Pending
}

fn stop_active_attempts(attempts: &mut [super::SetupAttemptV1], at: u64) {
    for attempt in attempts {
        if attempt.state == AttemptState::NeedsApproval {
            attempt.state = AttemptState::Cancelled;
            attempt.failure = Some(FailureCode::PrerequisiteRegressed);
            attempt.authorization_binding = None;
            attempt.owner_instance_id = None;
            attempt.updated_at_unix_ms = at;
        } else if matches!(
            attempt.state,
            AttemptState::InProgress | AttemptState::AwaitingVerification
        ) {
            attempt.state = AttemptState::Interrupted;
            attempt.failure = Some(FailureCode::PrerequisiteRegressed);
            attempt.authorization_binding = None;
            attempt.owner_instance_id = None;
            attempt.verified_result = None;
            attempt.updated_at_unix_ms = at;
        }
    }
}
