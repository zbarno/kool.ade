use crate::app::setup_attention::catalog::FindingStatus;

use super::super::{SetupTaskStateV1, SetupTaskStoreV1};
use super::support::{scan, snapshot};

#[test]
fn unknown_provider_suffix_does_not_verify_from_generic_provider_readiness() {
    let task_id = "setup:cli:codex:experimental";
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(&[(task_id, FindingStatus::NeedsAttention, &[])], &[], 1),
    )
    .unwrap();
    assert_eq!(store.tasks[task_id].state, SetupTaskStateV1::ReadyToFix);

    scan(&mut store, &snapshot(&[], &["cli:codex"], 2)).unwrap();

    assert_eq!(store.tasks[task_id].state, SetupTaskStateV1::ReadyToFix);
    assert!(store.tasks[task_id].verified_result.is_none());
}
