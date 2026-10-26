use std::collections::BTreeMap;

use crate::persistence::setup_tasks::{
    AttemptState, AuthorizationBindingV1, FailureCode, SetupAttemptV1, SetupTaskEventV1,
    SetupTaskStateV1, SetupTaskStoreV1, SetupTaskTransitionV1, SetupTaskV1,
    apply_transition_at_for_test, authorization_scope_digest, begin_scan_at_for_test,
    load_from_for_test, save_to_for_test,
};

use super::super::{AttemptContext, Controller};
use super::RecoveryAction;
use super::recovery::{
    AttemptResolution, recover_await_verification_write, settle_attempt_transition,
    settle_failed_attempt,
};
use super::steps::{PipelineFailureKind, authorize_publish_execute};

const TASK_ID: &str =
    "setup:images:general:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000001";
const RECEIPT_ID: &str = "00000000-0000-4000-8000-000000000002";
const NEXT_OPERATION_ID: &str = "00000000-0000-4000-8000-000000000003";

fn context(operation_id: &str) -> AttemptContext {
    AttemptContext {
        task_id: TASK_ID.into(),
        operation_id: operation_id.into(),
        version: 1,
        requested_scopes: vec![TASK_ID.into()],
    }
}

fn binding(context: &AttemptContext) -> AuthorizationBindingV1 {
    AuthorizationBindingV1 {
        receipt_id: RECEIPT_ID.into(),
        scope_digest: authorization_scope_digest(
            &context.task_id,
            &context.operation_id,
            context.version,
            &context.requested_scopes,
        ),
        authorized_at_unix_ms: 2,
    }
}

fn task() -> SetupTaskV1 {
    SetupTaskV1 {
        schema_version: 1,
        id: TASK_ID.into(),
        title: "Prepare image".into(),
        explanation: "The image needs verification.".into(),
        next_step: "Review image preparation.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state: SetupTaskStateV1::ReadyToFix,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: 1,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::<SetupAttemptV1>::new(),
        events: vec![SetupTaskEventV1 {
            observed_at_unix_ms: 1,
            scan_generation: 1,
            state: SetupTaskStateV1::ReadyToFix,
        }],
        legacy_source_id: None,
        superseded_by: Vec::new(),
    }
}

fn seed_store(path: &std::path::Path) {
    let store = SetupTaskStoreV1 {
        latest_scan_generation: 1,
        tasks: BTreeMap::from([(TASK_ID.into(), task())]),
        ..SetupTaskStoreV1::default()
    };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    save_to_for_test(path, &store).unwrap();
}

fn request(path: &std::path::Path, attempt: &AttemptContext) {
    apply_transition_at_for_test(
        path,
        SetupTaskTransitionV1::RequestAttempt {
            task_id: attempt.task_id.clone(),
            operation_id: attempt.operation_id.clone(),
            version: attempt.version,
            requested_scopes: attempt.requested_scopes.clone(),
            at_unix_ms: 1,
        },
    )
    .unwrap();
}

fn disk_store(path: &std::path::Path) -> SetupTaskStoreV1 {
    load_from_for_test(path).unwrap().0
}

/// Production boundary: invokes the same post-approval transition pipeline as
/// `Controller::begin_prepare`, with real locked disk transactions and a fake
/// executor that reads the durable InProgress record before it can run. A
/// validator unit test cannot prove controller ordering or stale-write
/// fail-closed behavior. Dependencies: temp directory, file lock, and fsync;
/// no Docker, network, or SSH. Setup cost is low.
#[test]
fn approval_is_durable_before_execution_and_stale_cancel_remains_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("machine-setup").join("tasks.json");
    seed_store(&path);
    let attempt = context(OPERATION_ID);
    request(&path, &attempt);
    let mut ran = 0;
    let result = authorize_publish_execute(
        &attempt,
        binding(&attempt),
        |transition| apply_transition_at_for_test(&path, transition),
        |snapshot| {
            assert_eq!(snapshot.tasks[TASK_ID].state, SetupTaskStateV1::InProgress);
            assert_eq!(disk_store(&path), snapshot);
            true
        },
        || {
            let durable = disk_store(&path);
            assert_eq!(durable.tasks[TASK_ID].state, SetupTaskStateV1::InProgress);
            assert_eq!(
                durable.tasks[TASK_ID].attempts[0].state,
                AttemptState::InProgress
            );
            assert!(
                durable.tasks[TASK_ID].attempts[0]
                    .authorization_binding
                    .is_some()
            );
            ran += 1;
            Ok(())
        },
    );
    assert!(result.is_ok());
    assert_eq!(ran, 1);
    assert_eq!(
        disk_store(&path).tasks[TASK_ID].state,
        SetupTaskStateV1::AwaitingVerification
    );

    let stale_temp = tempfile::tempdir().unwrap();
    let stale_path = stale_temp.path().join("machine-setup").join("tasks.json");
    seed_store(&stale_path);
    let stale = context(NEXT_OPERATION_ID);
    request(&stale_path, &stale);
    begin_scan_at_for_test(&stale_path).unwrap();
    let mut stale_execution_count = 0;
    let rejected = authorize_publish_execute(
        &stale,
        binding(&stale),
        |transition| apply_transition_at_for_test(&stale_path, transition),
        |_| panic!("stale approval must not be published"),
        || {
            stale_execution_count += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(rejected.kind, PipelineFailureKind::AuthorizationPersistence);
    assert_eq!(stale_execution_count, 0);

    let mut retry_write = true;
    let cancelled = settle_attempt_transition(
        &stale,
        RecoveryAction::Cancel,
        |transition| {
            if retry_write {
                retry_write = false;
                return Err(anyhow::anyhow!("injected transient write failure"));
            }
            apply_transition_at_for_test(&stale_path, transition)
        },
        || Ok(disk_store(&stale_path)),
    );
    assert_eq!(cancelled.resolution, AttemptResolution::Settled);
    assert_eq!(
        cancelled.store_update.as_ref().unwrap().tasks[TASK_ID].attempts[0].state,
        AttemptState::Cancelled
    );
    assert_eq!(
        disk_store(&stale_path).tasks[TASK_ID].attempts[0].failure,
        Some(FailureCode::CancelledByUser)
    );
    let mut controller = Controller {
        attempt: Some(stale.clone()),
        ..Controller::default()
    };
    controller.apply_resolution(cancelled.resolution);
    assert!(controller.attempt.is_none());

    let failure_temp = tempfile::tempdir().unwrap();
    let failure_path = failure_temp.path().join("machine-setup").join("tasks.json");
    seed_store(&failure_path);
    let failed = context("00000000-0000-4000-8000-000000000004");
    request(&failure_path, &failed);
    let mut report_after_commit = true;
    let failed_resolution = settle_failed_attempt(
        &failed,
        FailureCode::OperationFailed,
        |transition| {
            let persisted = apply_transition_at_for_test(&failure_path, transition)?;
            if report_after_commit {
                report_after_commit = false;
                return Err(anyhow::anyhow!("injected post-commit error"));
            }
            Ok(persisted)
        },
        || Ok(disk_store(&failure_path)),
    );
    assert_eq!(failed_resolution.resolution, AttemptResolution::Settled);
    assert_eq!(
        failed_resolution.store_update.as_ref().unwrap().tasks[TASK_ID].attempts[0].state,
        AttemptState::Failed
    );

    let retry_temp = tempfile::tempdir().unwrap();
    let retry_path = retry_temp.path().join("machine-setup").join("tasks.json");
    seed_store(&retry_path);
    let retry = context("00000000-0000-4000-8000-000000000005");
    request(&retry_path, &retry);
    let unresolved = settle_failed_attempt(
        &retry,
        FailureCode::OperationFailed,
        |_| Err(anyhow::anyhow!("injected persistent write failure")),
        || Ok(disk_store(&retry_path)),
    );
    assert_eq!(
        unresolved.resolution,
        AttemptResolution::Active(RecoveryAction::Fail(FailureCode::OperationFailed))
    );
    assert_eq!(
        unresolved.store_update.as_ref().unwrap().tasks[TASK_ID].attempts[0].state,
        AttemptState::NeedsApproval
    );
    controller.attempt = Some(retry.clone());
    controller.apply_resolution(unresolved.resolution);
    assert_eq!(
        controller.recovery_action,
        Some(RecoveryAction::Fail(FailureCode::OperationFailed))
    );

    let (verified, unresolved_verification) = recover_await_verification_write(
        &retry,
        || Err(anyhow::anyhow!("injected load failure")),
        |_| panic!("do not overwrite an unread success with failure"),
    );
    assert!(!verified);
    assert_eq!(
        unresolved_verification.resolution,
        AttemptResolution::Active(RecoveryAction::ReconcileVerification)
    );
    controller.apply_resolution(unresolved_verification.resolution);
    assert_eq!(
        controller.recovery_action,
        Some(RecoveryAction::ReconcileVerification)
    );
}
