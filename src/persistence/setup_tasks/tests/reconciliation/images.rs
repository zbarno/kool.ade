use crate::app::setup_attention::catalog::FindingStatus;

use super::super::support::{scan, snapshot};
use crate::persistence::setup_tasks::{
    AttemptState, AuthorizationBindingV1, FailureCode, SetupAttemptV1, SetupTaskEventV1,
    SetupTaskStateV1, SetupTaskStoreV1, SetupTaskV1, VerifiedResultV1, authorization_scope_digest,
};

#[test]
fn image_inventory_alone_never_verifies_a_preparation_task() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[
                (
                    "setup:images:general:amd64",
                    FindingStatus::NeedsAttention,
                    &[],
                ),
                (
                    "setup:images:rust:amd64",
                    FindingStatus::NeedsAttention,
                    &[],
                ),
            ],
            &[],
            1,
        ),
    )
    .unwrap();
    scan(&mut store, &snapshot(&[], &["images:rust:amd64"], 2)).unwrap();
    assert_eq!(
        store.tasks["setup:images:rust:amd64"].state,
        SetupTaskStateV1::ReadyToFix
    );
    assert_eq!(
        store.tasks["setup:images:general:amd64"].state,
        SetupTaskStateV1::ReadyToFix
    );
}

#[test]
fn matching_engine_inventory_still_needs_a_smoke_receipt() {
    let task_id = concat!(
        "setup:images:rust:amd64:",
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    );
    let domain = concat!(
        "images:rust:amd64:",
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    );
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(&[(task_id, FindingStatus::NeedsAttention, &[])], &[], 1),
    )
    .unwrap();
    scan(&mut store, &snapshot(&[], &[domain], 2)).unwrap();

    assert_eq!(store.tasks[task_id].state, SetupTaskStateV1::ReadyToFix);
    assert!(!store.has_valid_image_preparation_receipt(task_id));
}

#[test]
fn engine_switch_supersedes_old_verified_and_incomplete_image_tasks() {
    let old_verified_id = image_task_id("rust", 'a');
    let new_verified_id = image_task_id("rust", 'b');
    let old_incomplete_id = image_task_id("go", 'c');
    let new_incomplete_id = image_task_id("go", 'd');
    let old_verified = image_task(
        &old_verified_id,
        SetupTaskStateV1::Verified,
        Some(verified_attempt(&old_verified_id, 1)),
    );
    let old_incomplete = image_task(
        &old_incomplete_id,
        SetupTaskStateV1::ReadyToFix,
        Some(failed_attempt(&old_incomplete_id, 2)),
    );
    let verified_attempt_history = old_verified.attempts.clone();
    let incomplete_attempt_history = old_incomplete.attempts.clone();
    let mut store = SetupTaskStoreV1 {
        latest_scan_generation: 1,
        tasks: std::collections::BTreeMap::from([
            (old_verified_id.clone(), old_verified),
            (old_incomplete_id.clone(), old_incomplete),
        ]),
        ..SetupTaskStoreV1::default()
    };

    scan(
        &mut store,
        &snapshot(
            &[
                (&old_verified_id, FindingStatus::NeedsAttention, &[]),
                (&new_verified_id, FindingStatus::NeedsAttention, &[]),
                (&old_incomplete_id, FindingStatus::NeedsAttention, &[]),
                (&new_incomplete_id, FindingStatus::NeedsAttention, &[]),
            ],
            &[],
            20,
        ),
    )
    .unwrap();

    let superseded_verified = &store.tasks[&old_verified_id];
    assert_eq!(superseded_verified.state, SetupTaskStateV1::Superseded);
    assert_eq!(
        superseded_verified.superseded_by.as_slice(),
        std::slice::from_ref(&new_verified_id)
    );
    assert!(superseded_verified.verified_result.is_none());
    assert_eq!(superseded_verified.attempts, verified_attempt_history);

    let superseded_incomplete = &store.tasks[&old_incomplete_id];
    assert_eq!(superseded_incomplete.state, SetupTaskStateV1::Superseded);
    assert_eq!(
        superseded_incomplete.superseded_by.as_slice(),
        std::slice::from_ref(&new_incomplete_id)
    );
    assert!(superseded_incomplete.verified_result.is_none());
    assert_eq!(superseded_incomplete.attempts, incomplete_attempt_history);

    for current_id in [&new_verified_id, &new_incomplete_id] {
        assert_eq!(store.tasks[current_id].state, SetupTaskStateV1::ReadyToFix);
        assert!(store.tasks[current_id].superseded_by.is_empty());
        assert!(store.tasks[current_id].verified_result.is_none());
    }
}

#[test]
fn engine_switch_does_not_supersede_an_owned_in_progress_attempt() {
    let old_id = image_task_id("node", 'e');
    let current_id = image_task_id("node", 'f');
    let old_task = image_task(
        &old_id,
        SetupTaskStateV1::InProgress,
        Some(in_progress_attempt(&old_id, 3)),
    );
    let history = old_task.attempts.clone();
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

    let old = &store.tasks[&old_id];
    assert_eq!(old.state, SetupTaskStateV1::InProgress);
    assert!(old.superseded_by.is_empty());
    assert_eq!(old.attempts, history);
    assert_eq!(store.tasks[&current_id].state, SetupTaskStateV1::ReadyToFix);
    assert!(store.tasks[&current_id].superseded_by.is_empty());
}

fn image_task_id(profile: &str, identity_byte: char) -> String {
    format!(
        "setup:images:{profile}:amd64:{}",
        identity_byte.to_string().repeat(64)
    )
}

fn image_task(id: &str, state: SetupTaskStateV1, attempt: Option<SetupAttemptV1>) -> SetupTaskV1 {
    let verified_result = (state == SetupTaskStateV1::Verified).then(|| VerifiedResultV1 {
        evidence_domain: id.strip_prefix("setup:").unwrap().to_owned(),
        scan_generation: 1,
        verified_at_unix_ms: 10,
    });
    let event_states = if state == SetupTaskStateV1::Verified {
        vec![SetupTaskStateV1::ReadyToFix, state]
    } else {
        vec![state]
    };
    SetupTaskV1 {
        schema_version: 1,
        id: id.to_owned(),
        title: "Prepare the approved runtime image".into(),
        explanation: "Synthetic task from an older Docker Engine.".into(),
        next_step: "Review the current engine task.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state,
        created_at_unix_ms: 5,
        observed_at_unix_ms: 10,
        scan_generation: 1,
        verified_at_unix_ms: verified_result
            .as_ref()
            .map(|result| result.verified_at_unix_ms),
        verified_result,
        attempts: attempt.into_iter().collect(),
        events: event_states
            .into_iter()
            .map(|state| SetupTaskEventV1 {
                observed_at_unix_ms: 10,
                scan_generation: 1,
                state,
            })
            .collect(),
        legacy_source_id: None,
        superseded_by: Vec::new(),
    }
}

fn base_attempt(task_id: &str, number: u64, state: AttemptState) -> SetupAttemptV1 {
    let operation_id = format!("00000000-0000-4000-8000-{number:012}");
    SetupAttemptV1 {
        operation_id,
        version: 1,
        owner_instance_id: None,
        requested_scopes: vec![task_id.to_owned()],
        authorization_binding: None,
        authorization_history: None,
        state,
        started_at_unix_ms: 8,
        updated_at_unix_ms: 10,
        verified_result: None,
        remediation_result: None,
        image_preparation_receipt: None,
        failure: None,
    }
}

fn verified_attempt(task_id: &str, number: u64) -> SetupAttemptV1 {
    let mut attempt = base_attempt(task_id, number, AttemptState::Verified);
    attempt.verified_result = Some(VerifiedResultV1 {
        evidence_domain: task_id.strip_prefix("setup:").unwrap().to_owned(),
        scan_generation: 1,
        verified_at_unix_ms: 10,
    });
    attempt
}

fn failed_attempt(task_id: &str, number: u64) -> SetupAttemptV1 {
    let mut attempt = base_attempt(task_id, number, AttemptState::Failed);
    attempt.failure = Some(FailureCode::OperationFailed);
    attempt
}

fn in_progress_attempt(task_id: &str, number: u64) -> SetupAttemptV1 {
    let mut attempt = base_attempt(task_id, number, AttemptState::InProgress);
    let owner = "00000000-0000-4000-8000-000000000999";
    let scopes = vec![task_id.to_owned()];
    let binding = AuthorizationBindingV1 {
        receipt_id: "00000000-0000-4000-8000-000000000998".into(),
        scope_digest: authorization_scope_digest(task_id, &attempt.operation_id, 1, &scopes),
        authorized_at_unix_ms: 9,
    };
    attempt.owner_instance_id = Some(owner.into());
    attempt.authorization_binding = Some(binding.clone());
    attempt.authorization_history = Some(binding);
    attempt
}

#[test]
fn legacy_docker_image_findings_require_the_current_general_image_architecture() {
    for id in [
        "setup:docker:image-unverified",
        "setup:docker:image-digest",
        "setup:docker:image-architecture",
    ] {
        let status = if id == "setup:docker:image-unverified" {
            FindingStatus::Unverified
        } else {
            FindingStatus::Unsupported
        };
        let mut store = SetupTaskStoreV1::default();
        scan(&mut store, &snapshot(&[(id, status, &[])], &[], 1)).unwrap();
        scan(&mut store, &snapshot(&[], &["docker"], 2)).unwrap();
        let prior_state = if status == FindingStatus::Unsupported {
            SetupTaskStateV1::Unsupported
        } else {
            SetupTaskStateV1::NeedsVerification
        };
        assert_eq!(store.tasks[id].state, prior_state, "{id}");

        let mut wrong_architecture = snapshot(&[], &["images:general:arm64"], 3);
        wrong_architecture
            .capability_matrix
            .observed_host_architecture =
            crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64;
        scan(&mut store, &wrong_architecture).unwrap();
        assert_eq!(store.tasks[id].state, prior_state, "{id}");

        scan(&mut store, &snapshot(&[], &["images:general:amd64"], 4)).unwrap();
        assert_eq!(store.tasks[id].state, SetupTaskStateV1::Verified, "{id}");
    }
}
