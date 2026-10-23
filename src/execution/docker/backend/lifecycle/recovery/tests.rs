use super::*;
use crate::persistence::execution_records::{
    ExecutionCleanupStateV1, ExecutionRecordStateV1, ExecutionRecordV1,
};
use std::collections::HashMap;

fn record() -> ExecutionRecordV1 {
    ExecutionRecordV1 {
        schema_version: 1,
        execution_id: "00000000-0000-4000-8000-000000000001".into(),
        request: None,
        project_id: "00000000-0000-4000-8000-000000000002".into(),
        task_id: "00000000-0000-4000-8000-000000000003".into(),
        target_id: "local".into(),
        controller_target_id: None,
        owner_instance_id: "00000000-0000-4000-8000-000000000004".into(),
        container_name: Some("koolade-exec-00000000-0000-4000-8000-000000000001".into()),
        workspace_lease_id: None,
        workspace_source_sha: "a".repeat(40),
        destination_sha: None,
        workspace_manifest: None,
        container_id: None,
        command_id: None,
        command_started: false,
        result: None,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        policy_version: "docker-sandbox-v1".into(),
        lease_generation: 1,
        lease_expires_at_unix_ms: Some(2_000),
        attempt: 1,
        state: ExecutionRecordStateV1::Running,
        event_cursor: 2,
        output_bytes: 0,
        started_at_unix_ms: 1_000,
        updated_at_unix_ms: 1_001,
        ended_at_unix_ms: None,
        exit_code: None,
        artifacts: Vec::new(),
        docker_usage: crate::persistence::execution_records::DockerResourceUsageV1::default(),
        phase_timings: crate::persistence::execution_records::ExecutionPhaseTimingsV1::default(),
        cleanup: ExecutionCleanupStateV1::Pending,
    }
}

fn matching_labels(record: &ExecutionRecordV1) -> HashMap<String, String> {
    HashMap::from([
        (OWNER_LABEL.into(), record.owner_instance_id.clone()),
        (EXECUTION_LABEL.into(), record.execution_id.clone()),
        (TASK_LABEL.into(), record.task_id.clone()),
        (
            WORKSPACE_LABEL.into(),
            format!("{}:{}", record.project_id, record.task_id),
        ),
        (POLICY_LABEL.into(), record.policy_version.clone()),
        (
            LEASE_LABEL.into(),
            record.lease_expires_at_unix_ms.unwrap().to_string(),
        ),
    ])
}

#[test]
fn recovery_requires_a_complete_matching_ownership_label_set() {
    let record = record();
    let labels = matching_labels(&record);
    verify_record_labels(&record, &labels).unwrap();

    for key in [
        OWNER_LABEL,
        EXECUTION_LABEL,
        TASK_LABEL,
        WORKSPACE_LABEL,
        POLICY_LABEL,
        LEASE_LABEL,
    ] {
        let mut mismatched = labels.clone();
        mismatched.insert(key.into(), "different-identity".into());
        assert!(
            verify_record_labels(&record, &mismatched).is_err(),
            "mismatched ownership label {key} must be rejected"
        );
    }

    for key in [
        OWNER_LABEL,
        EXECUTION_LABEL,
        TASK_LABEL,
        WORKSPACE_LABEL,
        POLICY_LABEL,
        LEASE_LABEL,
    ] {
        let mut incomplete = labels.clone();
        incomplete.remove(key);
        assert!(
            verify_record_labels(&record, &incomplete).is_err(),
            "missing ownership label {key} must be rejected"
        );
    }
}

#[test]
fn recovery_waits_for_the_durable_lease_and_fails_closed_without_one() {
    let record = record();
    assert!(!lease_expired(&record, 1_999).unwrap());
    assert!(lease_expired(&record, 2_000).unwrap());

    let mut legacy = record;
    legacy.lease_expires_at_unix_ms = None;
    assert!(lease_expired(&legacy, u64::MAX).is_err());
}

#[test]
fn recovery_reconciles_a_terminal_journal_without_appending_after_it() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::test_support::KooladeHomeGuard::set(home.path());
    let mut record = record();
    record.state = ExecutionRecordStateV1::CollectingResults;
    record.event_cursor = 4;
    for (sequence, state) in [
        crate::execution::contract::ExecutionLifecycleV1::Queued,
        crate::execution::contract::ExecutionLifecycleV1::Preparing,
        crate::execution::contract::ExecutionLifecycleV1::Running,
        crate::execution::contract::ExecutionLifecycleV1::Collecting,
        crate::execution::contract::ExecutionLifecycleV1::Succeeded,
    ]
    .into_iter()
    .enumerate()
    {
        crate::persistence::execution_records::append_event(
            &record.task_id,
            &crate::execution::contract::ExecutionEventV1 {
                schema_version: 1,
                execution_id: record.execution_id.clone(),
                sequence: sequence as u64 + 1,
                timestamp_unix_ms: 1_001 + sequence as u64,
                event: crate::execution::contract::ExecutionEventKindV1::Lifecycle { state },
            },
        )
        .unwrap();
    }

    mark_interrupted(&mut record, false, None, "recovery test").unwrap();

    assert_eq!(record.state, ExecutionRecordStateV1::Succeeded);
    assert_eq!(record.event_cursor, 5);
    assert_eq!(record.exit_code, Some(0));
    assert!(record.result.is_some());
    let events = crate::persistence::execution_records::load_events(
        &record.task_id,
        &record.execution_id,
        crate::execution::contract::EventPageRequestV1::new(0, 256).unwrap(),
    )
    .unwrap();
    assert_eq!(events.len(), 5);
    assert!(matches!(
        events.last().unwrap().event,
        crate::execution::contract::ExecutionEventKindV1::Lifecycle {
            state: crate::execution::contract::ExecutionLifecycleV1::Succeeded
        }
    ));
    assert_eq!(
        crate::persistence::execution_records::load(&record.task_id, &record.execution_id)
            .unwrap()
            .unwrap(),
        record
    );
}

#[test]
fn terminal_reconciliation_preserves_an_already_persisted_execution_result() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::test_support::KooladeHomeGuard::set(home.path());
    let mut record = record();
    record.state = ExecutionRecordStateV1::Succeeded;
    record.event_cursor = 4;
    record.exit_code = Some(0);
    record.ended_at_unix_ms = Some(1_004);
    record.result = Some(crate::execution::contract::ExecutionResultV1 {
        schema_version: 1,
        execution_id: record.execution_id.clone(),
        exit_code: Some(0),
        signal: None,
        timed_out: false,
        cancelled: false,
        metrics: crate::execution::contract::ResourceMetricsV1 {
            cpu_millis: 123,
            peak_memory_bytes: 456,
            process_count: 7,
            disk_bytes: 89,
        },
        phase_timings: Default::default(),
        artifacts: Vec::new(),
        failure: None,
    });
    for (sequence, state) in [
        crate::execution::contract::ExecutionLifecycleV1::Queued,
        crate::execution::contract::ExecutionLifecycleV1::Preparing,
        crate::execution::contract::ExecutionLifecycleV1::Running,
        crate::execution::contract::ExecutionLifecycleV1::Collecting,
        crate::execution::contract::ExecutionLifecycleV1::Succeeded,
    ]
    .into_iter()
    .enumerate()
    {
        crate::persistence::execution_records::append_event(
            &record.task_id,
            &crate::execution::contract::ExecutionEventV1 {
                schema_version: 1,
                execution_id: record.execution_id.clone(),
                sequence: sequence as u64 + 1,
                timestamp_unix_ms: 1_001 + sequence as u64,
                event: crate::execution::contract::ExecutionEventKindV1::Lifecycle { state },
            },
        )
        .unwrap();
    }
    let persisted_result = record.result.clone();

    mark_interrupted(&mut record, true, Some(17), "recovery test").unwrap();

    assert_eq!(record.result, persisted_result);
    assert_eq!(record.cleanup, ExecutionCleanupStateV1::Complete);
    assert_eq!(record.phase_timings.cleanup_ms, Some(17));
}
