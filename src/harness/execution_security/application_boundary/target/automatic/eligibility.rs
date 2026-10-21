use crate::execution::contract::TargetPreference;

use super::super::ensure_target_allowed;

pub(super) fn local_target_eligible(
    local_ready: bool,
    allowed_targets: &[TargetPreference],
) -> bool {
    local_ready && ensure_target_allowed(&TargetPreference::Local, allowed_targets).is_ok()
}

pub(super) fn worker_target_allowed(worker_id: &str, allowed_targets: &[TargetPreference]) -> bool {
    ensure_target_allowed(
        &TargetPreference::Worker(worker_id.to_owned()),
        allowed_targets,
    )
    .is_ok()
}

pub(super) fn worker_retry_eligible(
    worker_id: &str,
    allowed_targets: &[TargetPreference],
    excluded_workers: &[String],
) -> bool {
    !excluded_workers
        .iter()
        .any(|excluded| excluded == worker_id)
        && worker_target_allowed(worker_id, allowed_targets)
}
