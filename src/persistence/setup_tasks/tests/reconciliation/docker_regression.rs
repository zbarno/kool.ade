use super::*;

/// Production boundary: setup scan reconciliation keeps an open Docker task
/// open when a later VM finding changes. This covers cross-task journal
/// behavior that isolated task-validator unit tests cannot establish.
/// External dependencies: none; the store is in memory. Setup cost: negligible.
#[test]
fn changed_docker_failure_does_not_verify_an_unrelated_task() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention, &[])],
            &[],
            1,
        ),
    )
    .unwrap();
    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:start", FindingStatus::NeedsAttention, &[])],
            &[],
            2,
        ),
    )
    .unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        SetupTaskStateV1::ReadyToFix
    );
}
