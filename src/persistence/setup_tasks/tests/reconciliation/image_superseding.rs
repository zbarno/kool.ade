use crate::app::setup_attention::catalog::FindingStatus;
use crate::persistence::setup_tasks::{
    AttemptState, AuthorizationBindingV1, FailureCode, SetupAttemptV1, SetupTaskEventV1,
    SetupTaskStateV1, SetupTaskStoreV1, SetupTaskV1, authorization_scope_digest,
};

use super::super::support::{scan, snapshot};

#[test]
fn stale_awaiting_verification_attempt_is_closed_when_engine_changes() {
    let old_id = image_task_id('a');
    let current_id = image_task_id('b');
    let operation_id = "00000000-0000-4000-8000-000000000001";
    let owner = "00000000-0000-4000-8000-000000000002";
    let scopes = vec![old_id.clone()];
    let binding = AuthorizationBindingV1 {
        receipt_id: "00000000-0000-4000-8000-000000000003".into(),
        scope_digest: authorization_scope_digest(&old_id, operation_id, 1, &scopes),
        authorized_at_unix_ms: 8,
    };
    let attempt = SetupAttemptV1 {
        operation_id: operation_id.into(),
        version: 1,
        owner_instance_id: Some(owner.into()),
        requested_scopes: scopes,
        authorization_binding: Some(binding.clone()),
        authorization_history: Some(binding),
        state: AttemptState::AwaitingVerification,
        started_at_unix_ms: 8,
        updated_at_unix_ms: 9,
        verified_result: None,
        remediation_result: None,
        image_preparation_receipt: None,
        failure: None,
    };
    let old_task = SetupTaskV1 {
        schema_version: 1,
        id: old_id.clone(),
        title: "Prepare image".into(),
        explanation: "Old engine identity".into(),
        next_step: "Use the current engine".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state: SetupTaskStateV1::AwaitingVerification,
        created_at_unix_ms: 5,
        observed_at_unix_ms: 9,
        scan_generation: 1,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: vec![attempt],
        events: vec![SetupTaskEventV1 {
            observed_at_unix_ms: 9,
            scan_generation: 1,
            state: SetupTaskStateV1::AwaitingVerification,
        }],
        legacy_source_id: None,
        superseded_by: Vec::new(),
    };
    let mut store = SetupTaskStoreV1 {
        latest_scan_generation: 1,
        tasks: std::collections::BTreeMap::from([(old_id.clone(), old_task)]),
        ..SetupTaskStoreV1::default()
    };

    scan(
        &mut store,
        &snapshot(
            &[(&current_id, FindingStatus::NeedsAttention, &[])],
            &[],
            20,
        ),
    )
    .unwrap();

    let stale = &store.tasks[&old_id];
    assert_eq!(stale.state, SetupTaskStateV1::Superseded);
    assert_eq!(
        stale.superseded_by.as_slice(),
        std::slice::from_ref(&current_id)
    );
    assert_eq!(stale.attempts[0].state, AttemptState::Failed);
    assert_eq!(
        stale.attempts[0].failure,
        Some(FailureCode::SupersededByReadiness)
    );
    assert!(stale.attempts[0].authorization_binding.is_none());
    assert!(stale.attempts[0].owner_instance_id.is_none());
    assert!(stale.attempts[0].authorization_history.is_some());
    assert_eq!(store.tasks[&current_id].state, SetupTaskStateV1::ReadyToFix);
    assert!(store.tasks[&current_id].superseded_by.is_empty());
}

#[test]
fn legacy_profile_task_is_superseded_by_a_current_engine_keyed_task() {
    let legacy_id = "setup:images:rust:amd64";
    let current_id = format!("{legacy_id}:{}", "b".repeat(64));
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(&[(legacy_id, FindingStatus::NeedsAttention, &[])], &[], 1),
    )
    .unwrap();
    scan(
        &mut store,
        &snapshot(
            &[
                (legacy_id, FindingStatus::NeedsAttention, &[]),
                (&current_id, FindingStatus::NeedsAttention, &[]),
            ],
            &[],
            2,
        ),
    )
    .unwrap();

    assert_eq!(store.tasks[legacy_id].state, SetupTaskStateV1::Superseded);
    assert_eq!(
        store.tasks[legacy_id].superseded_by.as_slice(),
        std::slice::from_ref(&current_id)
    );
    assert_eq!(store.tasks[&current_id].state, SetupTaskStateV1::ReadyToFix);
    assert!(store.tasks[&current_id].superseded_by.is_empty());
}

fn image_task_id(identity: char) -> String {
    format!(
        "setup:images:node:amd64:{}",
        identity.to_string().repeat(64)
    )
}
