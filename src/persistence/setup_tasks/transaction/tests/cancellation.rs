use super::*;
use crate::persistence::setup_tasks::{AttemptState, FailureCode, SetupTaskStateV1};
use std::cell::Cell;

/// Production boundary: cancellation after reconciliation must discard the
/// candidate snapshot while retaining independent restart recovery. A pure
/// state test cannot prove the persisted file stays at recovery-only state.
/// Uses a temporary local store; no Docker or network, low setup cost.
#[test]
fn cancellation_after_reconciliation_discards_candidate_but_persists_recovery() {
    const TASK_ID: &str = "setup:docker:rootless";
    const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000041";
    const OWNER_ID: &str = "00000000-0000-4000-8000-000000000042";

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let first_generation = begin_scan_at(&path).unwrap();
    update_at(&path, |store| {
        store.reconcile(
            &snapshot(first_generation, "original scan"),
            first_generation,
        )?;
        let scopes = vec![TASK_ID.to_owned()];
        store.request_attempt(TASK_ID, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)?;
        store.authorize_attempt(
            TASK_ID,
            OPERATION_ID,
            crate::persistence::setup_tasks::AuthorizationBindingV1 {
                receipt_id: "00000000-0000-4000-8000-000000000043".into(),
                scope_digest: crate::persistence::setup_tasks::authorization_scope_digest(
                    TASK_ID,
                    OPERATION_ID,
                    1,
                    &scopes,
                ),
                authorized_at_unix_ms: 3,
            },
            OWNER_ID,
            3,
        )?;
        store.await_verification(TASK_ID, OPERATION_ID, OWNER_ID, 4)?;
        Ok(((), true))
    })
    .unwrap();

    let generation = begin_scan_at(&path).unwrap();
    let candidate = snapshot(generation, "canceled candidate");
    let currentness_checks = Cell::new(0);
    let result = reconcile_at_if(&path, &candidate, generation, || {
        let check = currentness_checks.get();
        currentness_checks.set(check + 1);
        check < 2
    })
    .unwrap();

    assert!(result.is_none());
    assert_eq!(currentness_checks.get(), 3);
    let store = load_from(&path).unwrap().0;
    let task = &store.tasks[TASK_ID];
    assert_eq!(task.title, "original scan");
    assert_eq!(task.state, SetupTaskStateV1::Interrupted);
    assert_eq!(task.attempts[0].state, AttemptState::Interrupted);
    assert_eq!(
        task.attempts[0].failure,
        Some(FailureCode::InterruptedByRestart)
    );
    assert_eq!(store.latest_scan_generation, generation);
    assert_eq!(
        task.scan_generation,
        generation.saturating_sub(1),
        "restart recovery should remain persisted independently"
    );
}
