use crate::app::setup_attention::catalog::FindingStatus;

use super::super::{
    AttemptState, AuthorizationBindingV1, FailureCode, SetupTaskStateV1, authorization_scope_digest,
};
use super::support::{ready_store, rootless_smoke_fact, scan, snapshot};

const TASK_ID: &str = "setup:docker:rootless";
const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000001";
const NEXT_OPERATION_ID: &str = "00000000-0000-4000-8000-000000000003";
const RECEIPT_ID: &str = "00000000-0000-4000-8000-000000000002";
const OWNER_ID: &str = "00000000-0000-4000-8000-000000000004";
const RECOVERY_OWNER_ID: &str = "00000000-0000-4000-8000-000000000005";

fn binding(
    task_id: &str,
    operation_id: &str,
    version: u16,
    scopes: &[String],
) -> AuthorizationBindingV1 {
    AuthorizationBindingV1 {
        receipt_id: RECEIPT_ID.into(),
        scope_digest: authorization_scope_digest(task_id, operation_id, version, scopes),
        authorized_at_unix_ms: 3,
    }
}

#[test]
fn consent_metadata_binds_exact_attempt_and_verification_requires_fresh_evidence() {
    let mut store = ready_store(TASK_ID);
    let scopes = vec![TASK_ID.to_owned()];
    store
        .request_attempt(TASK_ID, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)
        .unwrap();
    assert_eq!(store.tasks[TASK_ID].state, SetupTaskStateV1::NeedsApproval);
    store
        .authorize_attempt(
            TASK_ID,
            OPERATION_ID,
            binding(TASK_ID, OPERATION_ID, 1, &scopes),
            OWNER_ID,
            3,
        )
        .unwrap();
    assert_eq!(store.tasks[TASK_ID].state, SetupTaskStateV1::InProgress);
    store
        .await_verification(TASK_ID, OPERATION_ID, OWNER_ID, 4)
        .unwrap();
    assert_eq!(
        store.tasks[TASK_ID].attempts[0].state,
        AttemptState::AwaitingVerification
    );

    let generation = store.latest_scan_generation + 1;
    let mut current = snapshot(&[], &["docker:rootless-smoke"], 5);
    current
        .probe_evidence
        .push(rootless_smoke_fact(generation, 5));
    scan(&mut store, &current).unwrap();
    let task = &store.tasks[TASK_ID];
    assert_eq!(task.state, SetupTaskStateV1::Verified);
    assert_eq!(task.attempts[0].state, AttemptState::Verified);
    assert_eq!(
        task.attempts[0]
            .verified_result
            .as_ref()
            .unwrap()
            .evidence_domain,
        "docker:rootless-smoke"
    );
    assert!(task.attempts[0].authorization_binding.is_none());
    assert!(task.attempts[0].authorization_history.is_some());
    store.validate().unwrap();
}

#[test]
fn fresh_attention_fails_an_attempt_that_did_not_pass_verification() {
    let mut store = ready_store(TASK_ID);
    let scopes = vec![TASK_ID.to_owned()];
    store
        .request_attempt(TASK_ID, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)
        .unwrap();
    store
        .authorize_attempt(
            TASK_ID,
            OPERATION_ID,
            binding(TASK_ID, OPERATION_ID, 1, &scopes),
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
    assert!(task.attempts[0].verified_result.is_none());
    store.validate().unwrap();
}

#[test]
fn preview_failure_and_cancel_are_redacted_before_authorization() {
    let mut store = ready_store(TASK_ID);
    let scopes = vec![TASK_ID.to_owned()];
    store
        .request_attempt(TASK_ID, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)
        .unwrap();
    store
        .fail_attempt(
            TASK_ID,
            OPERATION_ID,
            FailureCode::OperationFailed,
            OWNER_ID,
            3,
        )
        .unwrap();
    let failed = &store.tasks[TASK_ID].attempts[0];
    assert_eq!(failed.state, AttemptState::Failed);
    assert_eq!(failed.failure, Some(FailureCode::OperationFailed));
    assert!(failed.authorization_history.is_none());
    assert!(failed.owner_instance_id.is_none());
    store.validate().unwrap();

    scan(
        &mut store,
        &snapshot(&[(TASK_ID, FindingStatus::NeedsAttention, &[])], &[], 4),
    )
    .unwrap();
    store
        .request_attempt(TASK_ID, NEXT_OPERATION_ID.into(), 1, scopes, OWNER_ID, 5)
        .unwrap();
    store
        .cancel_attempt(TASK_ID, NEXT_OPERATION_ID, OWNER_ID, 6)
        .unwrap();
    let cancelled = &store.tasks[TASK_ID].attempts[1];
    assert_eq!(cancelled.state, AttemptState::Cancelled);
    assert_eq!(cancelled.failure, Some(FailureCode::CancelledByUser));
    assert!(cancelled.authorization_binding.is_none());
    assert!(cancelled.owner_instance_id.is_none());
    store.validate().unwrap();
}

#[test]
fn mismatched_or_reused_receipt_cannot_start_an_attempt() {
    let mut store = ready_store(TASK_ID);
    let scopes = vec![TASK_ID.to_owned()];
    store
        .request_attempt(TASK_ID, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)
        .unwrap();
    let mismatch = AuthorizationBindingV1 {
        receipt_id: RECEIPT_ID.into(),
        scope_digest: authorization_scope_digest(TASK_ID, NEXT_OPERATION_ID, 1, &scopes),
        authorized_at_unix_ms: 3,
    };
    assert!(
        store
            .authorize_attempt(TASK_ID, OPERATION_ID, mismatch, OWNER_ID, 3)
            .is_err()
    );
    assert_eq!(store.tasks[TASK_ID].state, SetupTaskStateV1::NeedsApproval);

    store
        .authorize_attempt(
            TASK_ID,
            OPERATION_ID,
            binding(TASK_ID, OPERATION_ID, 1, &scopes),
            OWNER_ID,
            3,
        )
        .unwrap();
    store
        .fail_attempt(
            TASK_ID,
            OPERATION_ID,
            FailureCode::OperationFailed,
            OWNER_ID,
            4,
        )
        .unwrap();
    assert_eq!(store.tasks[TASK_ID].state, SetupTaskStateV1::Failed);
    assert!(
        store.tasks[TASK_ID].attempts[0]
            .authorization_binding
            .is_none()
    );

    scan(
        &mut store,
        &snapshot(
            &[(TASK_ID, FindingStatus::NeedsAttention, &[])],
            &["docker:rootless"],
            5,
        ),
    )
    .unwrap();
    store
        .request_attempt(
            TASK_ID,
            NEXT_OPERATION_ID.into(),
            1,
            scopes.clone(),
            OWNER_ID,
            6,
        )
        .unwrap();
    assert!(
        store
            .authorize_attempt(
                TASK_ID,
                NEXT_OPERATION_ID,
                binding(TASK_ID, NEXT_OPERATION_ID, 1, &scopes),
                OWNER_ID,
                7
            )
            .is_err()
    );
    store.validate().unwrap();
}

#[test]
fn restart_interrupts_approval_and_operation_without_replaying_authority() {
    let mut store = ready_store(TASK_ID);
    let scopes = vec![TASK_ID.to_owned()];
    store
        .request_attempt(TASK_ID, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)
        .unwrap();
    store
        .authorize_attempt(
            TASK_ID,
            OPERATION_ID,
            binding(TASK_ID, OPERATION_ID, 1, &scopes),
            OWNER_ID,
            3,
        )
        .unwrap();
    assert!(store.recover_interrupted(4, RECOVERY_OWNER_ID, |_| false));
    let task = &store.tasks[TASK_ID];
    assert_eq!(task.state, SetupTaskStateV1::Interrupted);
    assert_eq!(task.attempts[0].state, AttemptState::Interrupted);
    assert_eq!(
        task.attempts[0].failure,
        Some(FailureCode::InterruptedByRestart)
    );
    assert!(task.attempts[0].authorization_binding.is_none());
    assert!(
        store
            .authorize_attempt(
                TASK_ID,
                OPERATION_ID,
                binding(TASK_ID, OPERATION_ID, 1, &scopes),
                RECOVERY_OWNER_ID,
                5
            )
            .is_err()
    );

    scan(
        &mut store,
        &snapshot(
            &[(TASK_ID, FindingStatus::NeedsAttention, &[])],
            &["docker:rootless"],
            6,
        ),
    )
    .unwrap();
    assert_eq!(store.tasks[TASK_ID].state, SetupTaskStateV1::ReadyToFix);
    store
        .request_attempt(
            TASK_ID,
            NEXT_OPERATION_ID.into(),
            1,
            scopes.clone(),
            OWNER_ID,
            7,
        )
        .unwrap();
    assert_eq!(
        store.tasks[TASK_ID].attempts[0].state,
        AttemptState::Interrupted
    );
    assert_eq!(
        store.tasks[TASK_ID].attempts[1].state,
        AttemptState::NeedsApproval
    );
    store.validate().unwrap();
}
