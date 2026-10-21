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

pub(super) fn ordered_remaining_workers(
    worker_order: &[String],
    observed_worker_ids: &[String],
    excluded_workers: &[String],
) -> Vec<String> {
    worker_order
        .iter()
        .filter(|worker_id| {
            !excluded_workers.contains(worker_id) && observed_worker_ids.contains(worker_id)
        })
        .cloned()
        .collect()
}

pub(super) fn select_from_remaining<T>(
    eligible_workers: &[String],
    select: impl FnOnce(&[String]) -> Option<T>,
) -> Option<T> {
    if eligible_workers.is_empty() {
        None
    } else {
        select(eligible_workers)
    }
}
