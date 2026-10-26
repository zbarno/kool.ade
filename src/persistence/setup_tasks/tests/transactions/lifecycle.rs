use super::super::super::{SetupTaskStateV1, SetupTaskTransitionV1 as Transition};
use super::*;

/// Production boundary: typed transitions must survive serialization and
/// reload through the locked file transaction. An in-memory test cannot prove
/// durable round-tripping or that each transition passes store validation.
/// Setup uses only a temporary directory and fsync; no external services, low
/// setup cost.
#[test]
fn typed_lifecycle_transitions_round_trip_through_locked_disk_transactions() {
    let temp = tempfile::tempdir().unwrap();
    let path = transaction::path_for_state_root(temp.path());
    seed_ready(&path, &[TASK_ID]);

    let requested = transaction::apply_transition_at(
        &path,
        Transition::RequestAttempt {
            task_id: TASK_ID.into(),
            operation_id: OPERATION_ID.into(),
            version: 1,
            requested_scopes: vec![TASK_ID.into()],
            at_unix_ms: 11,
        },
    )
    .unwrap();
    assert_eq!(
        requested.tasks[TASK_ID].state,
        SetupTaskStateV1::NeedsApproval
    );
    assert_eq!(transaction::load_from(&path).unwrap().0, requested);

    let authorized = transaction::apply_transition_at(
        &path,
        Transition::RecordAuthorization {
            task_id: TASK_ID.into(),
            operation_id: OPERATION_ID.into(),
            binding: binding(TASK_ID, OPERATION_ID),
            at_unix_ms: 12,
        },
    )
    .unwrap();
    assert_eq!(
        authorized.tasks[TASK_ID].state,
        SetupTaskStateV1::InProgress
    );
    assert!(
        authorized.tasks[TASK_ID].attempts[0]
            .authorization_binding
            .is_some()
    );

    let awaiting = transaction::apply_transition_at(
        &path,
        Transition::AwaitVerification {
            task_id: TASK_ID.into(),
            operation_id: OPERATION_ID.into(),
            at_unix_ms: 13,
        },
    )
    .unwrap();
    assert_eq!(
        awaiting.tasks[TASK_ID].state,
        SetupTaskStateV1::AwaitingVerification
    );
    assert_eq!(transaction::load_from(&path).unwrap().0, awaiting);
}
