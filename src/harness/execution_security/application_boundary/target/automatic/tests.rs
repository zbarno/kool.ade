use super::{
    local_target_eligible, ordered_remaining_workers, select_from_remaining, worker_retry_eligible,
    worker_target_allowed,
};
use crate::execution::contract::TargetPreference;

#[test]
fn auto_local_fallback_obeys_the_task_target_allowlist() {
    assert!(local_target_eligible(true, &[]));
    assert!(local_target_eligible(
        true,
        &[
            TargetPreference::Local,
            TargetPreference::Worker("worker-east".into())
        ]
    ));
    assert!(!local_target_eligible(
        true,
        &[TargetPreference::Worker("worker-east".into())]
    ));
    assert!(!local_target_eligible(false, &[]));
}

#[test]
fn auto_worker_pool_obeys_the_task_target_allowlist() {
    let allowed = [TargetPreference::Worker("worker-east".into())];
    assert!(worker_target_allowed("worker-east", &allowed));
    assert!(!worker_target_allowed("worker-west", &allowed));
    assert!(worker_target_allowed("worker-west", &[]));
}

#[test]
fn confirmed_lost_worker_is_excluded_from_automatic_retry_candidates() {
    let excluded = vec!["worker-east".into()];
    assert!(!worker_retry_eligible("worker-east", &[], &excluded));
    assert!(worker_retry_eligible("worker-west", &[], &excluded));
    let allowed = [TargetPreference::Worker("worker-north".into())];
    assert!(!worker_retry_eligible("worker-west", &allowed, &[]));
    assert!(worker_retry_eligible("worker-north", &allowed, &[]));
}

#[test]
fn automatic_worker_order_ends_when_all_observed_workers_are_excluded() {
    let worker_order = vec!["worker-east".into(), "worker-west".into()];
    let observed = worker_order.clone();
    let exhausted = worker_order.clone();
    assert_eq!(
        ordered_remaining_workers(&worker_order, &observed, &exhausted),
        Vec::<String>::new()
    );
    assert_eq!(
        ordered_remaining_workers(&worker_order, &observed, &["worker-east".into()]),
        vec!["worker-west"]
    );
    let mut selector_called = false;
    let target = select_from_remaining::<()>(
        &ordered_remaining_workers(&worker_order, &observed, &exhausted),
        |_| {
            selector_called = true;
            Some(())
        },
    );
    assert!(target.is_none());
    assert!(!selector_called);
}
