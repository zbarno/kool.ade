use crate::app::setup_attention::catalog::MachineSnapshot;
use crate::execution::docker::images::RuntimeProfile;

use super::super::{AttemptState, FailureCode, SetupTaskStateV1, SetupTaskV1, image_receipt};

pub(super) fn supersede_stale_image_task(
    task: &mut SetupTaskV1,
    snapshot: &MachineSnapshot,
    generation: u64,
) -> Option<bool> {
    let replacement = replacement_image_task_id(&task.id, snapshot)?;
    if task.attempts.iter().any(|attempt| {
        attempt.state == AttemptState::InProgress && attempt.owner_instance_id.is_some()
    }) {
        return None;
    }

    let previous_state = task.state;
    let previous_superseded_by = task.superseded_by.clone();
    let had_verified_result = task.verified_result.is_some();
    let previous_attempts = task.attempts.clone();
    settle_active_attempts(task, snapshot.checked_at_unix_ms);
    task.state = SetupTaskStateV1::Superseded;
    task.superseded_by = vec![replacement];
    task.verified_result = None;
    let meaningful_change = previous_state != task.state
        || previous_superseded_by != task.superseded_by
        || had_verified_result
        || previous_attempts != task.attempts;
    let metadata_changed = task.observed_at_unix_ms != snapshot.checked_at_unix_ms
        || task.scan_generation != generation;
    task.observed_at_unix_ms = snapshot.checked_at_unix_ms;
    task.scan_generation = generation;
    if meaningful_change {
        super::super::record(task, snapshot.checked_at_unix_ms, generation);
    }
    Some(meaningful_change || metadata_changed)
}

pub(super) fn supersede_seen_image_task(
    task: &mut SetupTaskV1,
    existing: &SetupTaskV1,
    snapshot: &MachineSnapshot,
    generation: u64,
) -> Option<bool> {
    if existing.state == SetupTaskStateV1::Superseded
        && replacement_image_task_id(&task.id, snapshot).is_some()
    {
        task.state = SetupTaskStateV1::Superseded;
        task.superseded_by.clone_from(&existing.superseded_by);
        task.verified_result = None;
    }
    let prior = task.clone();
    let changed = supersede_stale_image_task(task, snapshot, generation)?;
    let observation_changed = super::task_observation_changed(existing, task);
    let meaningful_change = prior.state != task.state
        || prior.superseded_by != task.superseded_by
        || prior.verified_result != task.verified_result
        || prior.attempts != task.attempts;
    if observation_changed && !meaningful_change {
        super::super::record(task, snapshot.checked_at_unix_ms, generation);
    }
    Some(changed || observation_changed)
}

fn settle_active_attempts(task: &mut SetupTaskV1, at: u64) {
    for attempt in &mut task.attempts {
        match attempt.state {
            AttemptState::NeedsApproval => {
                attempt.state = AttemptState::Cancelled;
                attempt.failure = Some(FailureCode::SupersededByReadiness);
            }
            AttemptState::InProgress | AttemptState::AwaitingVerification => {
                attempt.state = AttemptState::Failed;
                attempt.failure = Some(FailureCode::SupersededByReadiness);
                attempt.verified_result = None;
            }
            _ => continue,
        }
        attempt.authorization_binding = None;
        attempt.owner_instance_id = None;
        attempt.updated_at_unix_ms = at;
    }
}

pub(super) fn replacement_image_task_id(
    old_id: &str,
    snapshot: &MachineSnapshot,
) -> Option<String> {
    let old = image_task_scope(old_id)?;
    let mut replacements = std::collections::BTreeSet::new();
    for current_id in snapshot.findings.iter().map(|finding| finding.id.as_str()) {
        if is_different_image_identity(current_id, old) {
            replacements.insert(current_id.to_owned());
        }
    }
    for domain in &snapshot.checked_domains {
        let Some(image_domain) = domain.strip_prefix("images:") else {
            continue;
        };
        let current_id = format!("setup:images:{image_domain}");
        if is_different_image_identity(&current_id, old) {
            replacements.insert(current_id);
        }
    }
    (replacements.len() == 1)
        .then(|| replacements.into_iter().next())
        .flatten()
}

fn is_different_image_identity(candidate_id: &str, old: ImageTaskScope<'_>) -> bool {
    let Some(candidate) = image_receipt::image_task_identity(candidate_id) else {
        return false;
    };
    candidate.profile == old.profile
        && candidate.architecture == old.architecture
        && Some(candidate.engine_identity_fingerprint) != old.engine_identity_fingerprint
}

#[derive(Clone, Copy)]
struct ImageTaskScope<'a> {
    profile: RuntimeProfile,
    architecture: &'a str,
    engine_identity_fingerprint: Option<&'a str>,
}

fn image_task_scope(task_id: &str) -> Option<ImageTaskScope<'_>> {
    let segments = task_id.split(':').collect::<Vec<_>>();
    if segments.len() == 5 {
        let identity = image_receipt::image_task_identity(task_id)?;
        return Some(ImageTaskScope {
            profile: identity.profile,
            architecture: identity.architecture,
            engine_identity_fingerprint: Some(identity.engine_identity_fingerprint),
        });
    }
    if segments.len() != 4 || segments[0] != "setup" || segments[1] != "images" {
        return None;
    }
    let profile = match segments[2] {
        "general" => RuntimeProfile::General,
        "rust" => RuntimeProfile::Rust,
        "node" => RuntimeProfile::Node,
        "python" => RuntimeProfile::Python,
        "go" => RuntimeProfile::Go,
        _ => return None,
    };
    let architecture = segments[3];
    if !matches!(architecture, "amd64" | "arm64") {
        return None;
    }
    Some(ImageTaskScope {
        profile,
        architecture,
        engine_identity_fingerprint: None,
    })
}
