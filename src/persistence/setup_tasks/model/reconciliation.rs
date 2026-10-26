use super::*;
use crate::app::setup_attention::catalog::MachineSnapshot;
use crate::persistence::setup_tasks::{MAX_TASKS, graph, valid_id};

mod image_superseding;

impl SetupTaskStoreV1 {
    /// Reconcile a read-only snapshot reserved by `begin_scan`.
    pub fn reconcile(
        &mut self,
        snapshot: &MachineSnapshot,
        generation: u64,
    ) -> anyhow::Result<bool> {
        let mut next = self.clone();
        let changed = next.reconcile_inner(snapshot, generation)?;
        next.validate()?;
        *self = next;
        Ok(changed)
    }

    fn reconcile_inner(
        &mut self,
        snapshot: &MachineSnapshot,
        generation: u64,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(
            snapshot.schema_version == SNAPSHOT_VERSION,
            "unsupported readiness snapshot version"
        );
        anyhow::ensure!(generation > 0, "setup scan generation must be positive");
        anyhow::ensure!(
            generation == self.latest_scan_generation,
            "stale machine setup scan generation"
        );
        anyhow::ensure!(
            snapshot.scan_generation == generation,
            "machine readiness snapshot generation does not match its reservation"
        );
        anyhow::ensure!(
            snapshot.findings.len() <= MAX_TASKS,
            "too many setup findings"
        );

        let mut changed = false;
        let mut seen = std::collections::BTreeSet::new();
        for finding in &snapshot.findings {
            anyhow::ensure!(valid_id(&finding.id), "invalid setup prerequisite ID");
            anyhow::ensure!(
                seen.insert(finding.id.clone()),
                "duplicate setup prerequisite ID"
            );
            for prerequisite in &finding.prerequisites {
                anyhow::ensure!(valid_id(prerequisite), "invalid setup dependency ID");
            }

            let mut next =
                SetupTaskV1::from_finding(finding, generation, snapshot.checked_at_unix_ms);
            let finding_state = next.state;
            if let Some(existing) = self.tasks.get(&finding.id).cloned() {
                next.created_at_unix_ms = existing.created_at_unix_ms;
                next.verified_at_unix_ms = existing.verified_at_unix_ms;
                next.attempts.clone_from(&existing.attempts);
                next.events.clone_from(&existing.events);
                preserve_dependency_resolutions(&existing, &mut next);
                preserve_inflight_state(&existing, &mut next);
                if let Some(superseding_changed) = image_superseding::supersede_seen_image_task(
                    &mut next, &existing, snapshot, generation,
                ) {
                    changed |= superseding_changed;
                    self.tasks.insert(finding.id.clone(), next);
                    continue;
                }
                if existing.state == SetupTaskStateV1::AwaitingVerification {
                    fail_awaiting_attempt(&mut next, finding_state, snapshot.checked_at_unix_ms);
                }

                let observation_changed = task_observation_changed(&existing, &next);
                let metadata_changed = existing.observed_at_unix_ms != snapshot.checked_at_unix_ms
                    || existing.scan_generation != generation;
                if observation_changed {
                    record(&mut next, snapshot.checked_at_unix_ms, generation);
                    changed = true;
                } else if metadata_changed {
                    changed = true;
                }
            } else {
                changed = true;
            }
            self.tasks.insert(finding.id.clone(), next);
        }

        for task in self.tasks.values_mut() {
            if seen.contains(task.id.as_str()) || task.state == SetupTaskStateV1::Superseded {
                continue;
            }
            if task.legacy_source_id.is_some() {
                let superseded_by = snapshot
                    .findings
                    .iter()
                    .filter(|finding| legacy_matches(task.id.as_str(), finding.id.as_str()))
                    .map(|finding| finding.id.clone())
                    .collect::<Vec<_>>();
                if !superseded_by.is_empty() || positive_domain_for_task(task, snapshot).is_some() {
                    let meaningful = task.state != SetupTaskStateV1::Superseded
                        || task.superseded_by != superseded_by;
                    task.state = SetupTaskStateV1::Superseded;
                    task.superseded_by = superseded_by;
                    task.verified_result = None;
                    task.observed_at_unix_ms = snapshot.checked_at_unix_ms;
                    task.scan_generation = generation;
                    if meaningful {
                        record(task, snapshot.checked_at_unix_ms, generation);
                    }
                    changed |= meaningful;
                }
                continue;
            }

            if let Some(superseding_changed) =
                image_superseding::supersede_stale_image_task(task, snapshot, generation)
            {
                changed |= superseding_changed;
                continue;
            }

            if let Some(domain) = positive_domain_for_task(task, snapshot) {
                if task
                    .attempts
                    .iter()
                    .any(|attempt| attempt.state == AttemptState::InProgress)
                {
                    continue;
                }
                let was_verified = task.state == SetupTaskStateV1::Verified;
                if task.state == SetupTaskStateV1::NeedsApproval {
                    cancel_unapproved_attempts(task, snapshot.checked_at_unix_ms);
                }
                let result = VerifiedResultV1 {
                    evidence_domain: domain,
                    scan_generation: generation,
                    verified_at_unix_ms: snapshot.checked_at_unix_ms,
                };
                let state_changed = !was_verified;
                let receipt_changed = task.verified_result.as_ref() != Some(&result);
                let metadata_changed = task.observed_at_unix_ms != snapshot.checked_at_unix_ms
                    || task.scan_generation != generation;
                task.state = SetupTaskStateV1::Verified;
                task.verified_at_unix_ms = Some(snapshot.checked_at_unix_ms);
                task.verified_result = Some(result.clone());
                task.observed_at_unix_ms = snapshot.checked_at_unix_ms;
                task.scan_generation = generation;
                verify_awaiting_attempt(task, &result, snapshot.checked_at_unix_ms);
                if state_changed {
                    record(task, snapshot.checked_at_unix_ms, generation);
                }
                changed |= state_changed || receipt_changed || metadata_changed;
                continue;
            }

            // A previous success becomes uncertain if this scan could not check
            // the relevant domain. Keep its timestamp as history, not as a
            // current readiness receipt.
            if task.state == SetupTaskStateV1::Verified
                && super::validation::has_expected_evidence_domain(task.id.as_str())
            {
                task.state = SetupTaskStateV1::NeedsVerification;
                task.verified_result = None;
                task.observed_at_unix_ms = snapshot.checked_at_unix_ms;
                task.scan_generation = generation;
                record(task, snapshot.checked_at_unix_ms, generation);
                changed = true;
            }
        }

        changed |= graph::refresh_dependencies(self, snapshot, generation)?;
        anyhow::ensure!(
            self.tasks.len() <= MAX_TASKS,
            "setup task store reached its size limit"
        );
        Ok(changed)
    }
}

fn positive_domain_for_task(task: &SetupTaskV1, snapshot: &MachineSnapshot) -> Option<String> {
    let domain = positive_domain_for(task.id.as_str(), snapshot)?;
    if super::is_image_task_id(task.id.as_str())
        && !super::image_receipt::has_valid_image_preparation_receipt_for_task(
            task.id.as_str(),
            task.attempts.as_slice(),
        )
    {
        return None;
    }
    Some(domain)
}

fn preserve_dependency_resolutions(existing: &SetupTaskV1, next: &mut SetupTaskV1) {
    let previous = existing
        .dependencies
        .iter()
        .map(|dependency| (dependency.prerequisite_id.as_str(), &dependency.resolution))
        .collect::<std::collections::BTreeMap<_, _>>();
    for dependency in &mut next.dependencies {
        if let Some(resolution) = previous.get(dependency.prerequisite_id.as_str()) {
            dependency.resolution.clone_from(resolution);
        }
    }
}

fn preserve_inflight_state(existing: &SetupTaskV1, next: &mut SetupTaskV1) {
    next.verified_result = None;
    if matches!(
        existing.state,
        SetupTaskStateV1::NeedsApproval
            | SetupTaskStateV1::InProgress
            | SetupTaskStateV1::AwaitingVerification
            | SetupTaskStateV1::Paused
    ) {
        next.state = existing.state;
        next.verified_result.clone_from(&existing.verified_result);
    } else if (existing.state == SetupTaskStateV1::ReadyToFix
        || matches!(
            existing.state,
            SetupTaskStateV1::Failed | SetupTaskStateV1::Cancelled | SetupTaskStateV1::Interrupted
        ))
        && next.state == SetupTaskStateV1::Detected
    {
        next.state = SetupTaskStateV1::ReadyToFix;
    }
}

fn cancel_unapproved_attempts(task: &mut SetupTaskV1, at: u64) {
    for attempt in &mut task.attempts {
        if attempt.state == AttemptState::NeedsApproval {
            attempt.state = AttemptState::Cancelled;
            attempt.failure = Some(FailureCode::SupersededByReadiness);
            attempt.authorization_binding = None;
            attempt.owner_instance_id = None;
            attempt.updated_at_unix_ms = at;
        }
    }
}

fn verify_awaiting_attempt(task: &mut SetupTaskV1, result: &VerifiedResultV1, at: u64) {
    if let Some(attempt) = task
        .attempts
        .iter_mut()
        .rev()
        .find(|attempt| attempt.state == AttemptState::AwaitingVerification)
    {
        attempt.state = AttemptState::Verified;
        attempt.updated_at_unix_ms = at;
        attempt.verified_result = Some(result.clone());
        attempt.authorization_binding = None;
        attempt.owner_instance_id = None;
        attempt.failure = None;
    }
}

fn fail_awaiting_attempt(task: &mut SetupTaskV1, finding_state: SetupTaskStateV1, at: u64) {
    let Some(attempt) = task
        .attempts
        .iter_mut()
        .rev()
        .find(|attempt| attempt.state == AttemptState::AwaitingVerification)
    else {
        return;
    };
    attempt.state = AttemptState::Failed;
    attempt.updated_at_unix_ms = at;
    attempt.failure = Some(FailureCode::VerificationFailed);
    attempt.authorization_binding = None;
    attempt.owner_instance_id = None;
    attempt.verified_result = None;
    task.state = if finding_state == SetupTaskStateV1::Detected {
        SetupTaskStateV1::Failed
    } else {
        finding_state
    };
    task.verified_result = None;
}

fn task_observation_changed(existing: &SetupTaskV1, next: &SetupTaskV1) -> bool {
    existing.title != next.title
        || existing.explanation != next.explanation
        || existing.next_step != next.next_step
        || existing.dependencies != next.dependencies
        || existing.state != next.state
        || existing.verified_result != next.verified_result
        || existing.attempts != next.attempts
}

fn legacy_matches(legacy_id: &str, current_id: &str) -> bool {
    (legacy_id == "setup:legacy:docker" && current_id.starts_with("setup:docker:"))
        || (legacy_id == "setup:legacy:provider" && current_id.starts_with("setup:cli:"))
}
