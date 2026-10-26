use crate::app::setup_attention::catalog::FindingStatus;

use super::super::support::{ready_store, scan, snapshot};
use crate::persistence::setup_tasks::{
    AttemptState, AuthorizationBindingV1, FailureCode, SetupTaskStateV1, authorization_scope_digest,
};

#[test]
fn a_still_present_finding_fails_awaiting_verification_and_allows_retry() {
    const TASK_ID: &str = "setup:docker:rootless";
    const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000031";
    const NEXT_OPERATION_ID: &str = "00000000-0000-4000-8000-000000000032";
    const OWNER_ID: &str = "00000000-0000-4000-8000-000000000033";
    let mut store = ready_store(TASK_ID);
    let scopes = vec![TASK_ID.to_owned()];
    store
        .request_attempt(TASK_ID, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)
        .unwrap();
    store
        .authorize_attempt(
            TASK_ID,
            OPERATION_ID,
            AuthorizationBindingV1 {
                receipt_id: "00000000-0000-4000-8000-000000000034".into(),
                scope_digest: authorization_scope_digest(TASK_ID, OPERATION_ID, 1, &scopes),
                authorized_at_unix_ms: 3,
            },
            OWNER_ID,
            3,
        )
        .unwrap();
    store
        .await_verification(TASK_ID, OPERATION_ID, OWNER_ID, 4)
        .unwrap();

    scan(
        &mut store,
        &snapshot(&[(TASK_ID, FindingStatus::NeedsAttention, &[])], &[], 5),
    )
    .unwrap();
    let task = &store.tasks[TASK_ID];
    assert_eq!(task.state, SetupTaskStateV1::Failed);
    assert_eq!(task.attempts[0].state, AttemptState::Failed);
    assert_eq!(
        task.attempts[0].failure,
        Some(FailureCode::VerificationFailed)
    );
    assert!(task.attempts[0].authorization_binding.is_none());
    assert!(task.attempts[0].owner_instance_id.is_none());

    store
        .request_attempt(TASK_ID, NEXT_OPERATION_ID.into(), 1, scopes, OWNER_ID, 6)
        .unwrap();
    assert_eq!(
        store.tasks[TASK_ID].attempts[1].state,
        AttemptState::NeedsApproval
    );
    store.validate().unwrap();
}
