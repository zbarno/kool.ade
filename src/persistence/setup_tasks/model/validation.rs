mod attempts;

use super::*;
use crate::persistence::setup_tasks::{MAX_EVENTS, MAX_TASKS, graph, valid_id};
pub(super) use attempts::{
    desktop_docker_evidence, expected_evidence_domain, has_expected_evidence_domain,
};
use attempts::{validate_attempts, validate_result};

impl SetupTaskStoreV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == FILE_SCHEMA_VERSION,
            "unsupported setup store schema"
        );
        anyhow::ensure!(self.tasks.len() <= MAX_TASKS, "oversized setup task store");
        anyhow::ensure!(
            self.required_image_profiles.len()
                <= super::image_requirements::MAX_REQUIRED_IMAGE_PROFILES,
            "too many required runtime image profiles"
        );
        anyhow::ensure!(
            self.required_image_profiles
                .iter()
                .all(|profile| super::image_requirements::supported_profile(*profile)),
            "runtime QA image profiles cannot be required for bootstrap"
        );
        let mut operation_ids = std::collections::BTreeSet::new();
        let mut receipt_ids = std::collections::BTreeSet::new();

        for (id, task) in &self.tasks {
            validate_task_id(id, task, self.latest_scan_generation)?;
            validate_dependencies(id, task, self)?;
            validate_attempts(id, task, self, &mut operation_ids, &mut receipt_ids)?;
            validate_task_state(task, self.latest_scan_generation)?;
        }
        graph::ordered_task_ids(self)?;
        Ok(())
    }
}

fn validate_task_id(id: &str, task: &SetupTaskV1, latest_generation: u64) -> anyhow::Result<()> {
    anyhow::ensure!(valid_id(id) && id == task.id, "invalid saved setup task ID");
    anyhow::ensure!(
        task.schema_version == TASK_SCHEMA_VERSION,
        "unsupported setup task version"
    );
    anyhow::ensure!(
        task.title.len() <= 240 && task.explanation.len() <= 2_000 && task.next_step.len() <= 1_000,
        "setup task text is oversized"
    );
    anyhow::ensure!(
        task.dependencies.len() <= 64 && task.blocked_by.len() <= 64,
        "too many setup dependencies"
    );
    anyhow::ensure!(
        task.attempts.len() <= crate::persistence::setup_tasks::MAX_ATTEMPTS,
        "too many setup attempts"
    );
    anyhow::ensure!(
        task.events.len() <= MAX_EVENTS,
        "too many setup task events"
    );
    anyhow::ensure!(
        task.scan_generation <= latest_generation,
        "task refers to a future scan"
    );
    anyhow::ensure!(
        task.events
            .iter()
            .all(|event| event.scan_generation <= latest_generation),
        "task event refers to a future scan"
    );
    Ok(())
}

fn validate_dependencies(
    _id: &str,
    task: &SetupTaskV1,
    store: &SetupTaskStoreV1,
) -> anyhow::Result<()> {
    let mut dependency_ids = std::collections::BTreeSet::new();
    for dependency in &task.dependencies {
        anyhow::ensure!(
            valid_id(&dependency.prerequisite_id),
            "invalid setup dependency ID"
        );
        anyhow::ensure!(
            task.id != dependency.prerequisite_id,
            "setup task cannot depend on itself"
        );
        anyhow::ensure!(
            dependency_ids.insert(&dependency.prerequisite_id),
            "duplicate setup dependency"
        );
        match &dependency.resolution {
            DependencyResolution::Pending => {}
            DependencyResolution::SatisfiedByTask { scan_generation } => {
                let parent = store
                    .tasks
                    .get(&dependency.prerequisite_id)
                    .ok_or_else(|| anyhow::anyhow!("resolved setup task dependency is missing"))?;
                anyhow::ensure!(
                    parent.state == SetupTaskStateV1::Verified,
                    "resolved setup task dependency is not verified"
                );
                anyhow::ensure!(
                    parent
                        .verified_result
                        .as_ref()
                        .is_some_and(|receipt| receipt.scan_generation == *scan_generation),
                    "resolved setup task dependency has stale evidence"
                );
                anyhow::ensure!(
                    *scan_generation > 0 && *scan_generation <= store.latest_scan_generation,
                    "setup task dependency refers to invalid evidence"
                );
            }
            DependencyResolution::SatisfiedByProbe {
                evidence_domain,
                scan_generation,
                ..
            } => {
                anyhow::ensure!(
                    expected_evidence_domain(&dependency.prerequisite_id).as_deref()
                        == Some(evidence_domain.as_str()),
                    "setup dependency has mismatched probe evidence"
                );
                anyhow::ensure!(
                    *scan_generation > 0 && *scan_generation <= store.latest_scan_generation,
                    "setup dependency refers to invalid scan evidence"
                );
            }
        }
    }
    let unresolved = task
        .dependencies
        .iter()
        .filter(|dependency| dependency.resolution == DependencyResolution::Pending)
        .map(|dependency| dependency.prerequisite_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let blocked = task
        .blocked_by
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(
        unresolved == blocked && blocked.len() == task.blocked_by.len(),
        "setup blocked-by projection is inconsistent"
    );
    Ok(())
}

fn validate_task_state(task: &SetupTaskV1, latest_generation: u64) -> anyhow::Result<()> {
    let active_attempts = task
        .attempts
        .iter()
        .filter(|attempt| {
            matches!(
                attempt.state,
                AttemptState::NeedsApproval
                    | AttemptState::InProgress
                    | AttemptState::AwaitingVerification
            )
        })
        .collect::<Vec<_>>();
    let expected_active = match task.state {
        SetupTaskStateV1::NeedsApproval => Some(AttemptState::NeedsApproval),
        SetupTaskStateV1::InProgress => Some(AttemptState::InProgress),
        SetupTaskStateV1::AwaitingVerification => Some(AttemptState::AwaitingVerification),
        _ => None,
    };
    if let Some(expected) = expected_active {
        anyhow::ensure!(
            active_attempts.len() == 1 && active_attempts[0].state == expected,
            "task state does not match its active attempt"
        );
    } else {
        anyhow::ensure!(
            active_attempts.is_empty(),
            "task retains an active attempt in a terminal state"
        );
    }
    if task.state == SetupTaskStateV1::Verified {
        anyhow::ensure!(
            task.blocked_by.is_empty(),
            "verified task has unresolved prerequisites"
        );
        let result = task
            .verified_result
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("verified setup task lacks probe evidence"))?;
        validate_result(&task.id, result, latest_generation)?;
    } else {
        anyhow::ensure!(
            task.verified_result.is_none(),
            "unverified setup task retains positive probe evidence"
        );
    }
    if task.state == SetupTaskStateV1::BlockedByPrerequisite {
        anyhow::ensure!(
            !task.blocked_by.is_empty(),
            "blocked setup task has no unresolved prerequisite"
        );
    }
    Ok(())
}
