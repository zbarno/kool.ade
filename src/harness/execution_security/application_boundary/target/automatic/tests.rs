use super::{local_target_eligible, worker_target_allowed};
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
