use std::fs;

use super::*;

fn record() -> ExecutionRecordV1 {
    ExecutionRecordV1 {
        schema_version: super::record::SCHEMA_VERSION,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        request: None,
        project_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        target_id: "local".into(),
        controller_target_id: None,
        owner_instance_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        container_name: Some("koolade-exec-test".into()),
        workspace_lease_id: None,
        workspace_source_sha: "a".repeat(40),
        destination_sha: None,
        workspace_manifest: None,
        container_id: None,
        command_id: None,
        command_started: false,
        result: None,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        policy_version: "docker-policy-v1".into(),
        lease_generation: 1,
        lease_expires_at_unix_ms: Some(2_000),
        attempt: 1,
        state: ExecutionRecordStateV1::Preparing,
        event_cursor: 0,
        output_bytes: 0,
        started_at_unix_ms: 10,
        updated_at_unix_ms: 10,
        ended_at_unix_ms: None,
        exit_code: None,
        artifacts: Vec::new(),
        cleanup: ExecutionCleanupStateV1::Pending,
    }
}

#[test]
fn execution_records_validate_and_round_trip_outside_the_repository() {
    let root =
        std::env::temp_dir().join(format!("koolade-execution-record-{}", uuid::Uuid::new_v4()));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let mut record = record();
    let owner = owner_instance_id().unwrap();
    assert_eq!(owner, owner_instance_id().unwrap());
    record.owner_instance_id = owner.clone();
    let task_lock = try_task_lock(&record.task_id).unwrap().unwrap();
    assert!(try_task_lock(&record.task_id).unwrap().is_none());
    drop(task_lock);
    assert!(try_task_lock(&record.task_id).unwrap().is_some());
    save(&record).unwrap();
    assert_eq!(unfinished_for_owner(&owner).unwrap(), vec![record.clone()]);
    assert_eq!(
        load(&record.task_id, &record.execution_id).unwrap(),
        Some(record.clone())
    );
    assert!(root.join("executions").join(&record.task_id).exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn remote_execution_record_binds_controller_target_without_changing_worker_target() {
    let mut record = record();
    let worker_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let budget = crate::execution::contract::ResourceBudgetV1 {
        cpu_millis: 1_000,
        memory_bytes: 512 * 1024 * 1024,
        process_count: 32,
        temporary_disk_bytes: 64 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    };
    let mode = crate::execution::contract::ExecutionModeV1::Implementation;
    record.target_id = "local".into();
    record.controller_target_id = Some(worker_id.clone());
    record.request = Some(crate::execution::contract::ExecutionRequestV1 {
        schema_version: 1,
        execution_id: record.execution_id.clone(),
        authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        project_id: record.project_id.clone(),
        task_id: record.task_id.clone(),
        target: crate::execution::contract::ExecutionTarget::local(),
        controller_target_id: Some(worker_id.clone()),
        workspace: crate::execution::contract::WorkspaceRef {
            project_id: record.project_id.clone(),
            task_id: record.task_id.clone(),
            source_sha: record.workspace_source_sha.clone(),
        },
        mode,
        image_digest: record.image_digest.clone(),
        argv: vec!["sh".into(), "-s".into()],
        working_directory_relative: ".".into(),
        stdin: Some("true".into()),
        environment_allowlist: Default::default(),
        resource_budget: budget.clone(),
        deadline_unix_ms: 20_000,
        allowed_service_ports: Default::default(),
        broker_capability_ids: Default::default(),
        dependency_cache_refs: Vec::new(),
        cancellation_generation: record.lease_generation,
        request_sequence: 1,
        policy: crate::execution::policy::ExecutionPolicyV1::for_mode(
            mode,
            budget,
            Default::default(),
        )
        .unwrap(),
    });
    assert!(record.validate().is_ok());
    record.request.as_mut().unwrap().controller_target_id = None;
    assert!(record.validate().is_err());
}

#[test]
fn execution_event_journal_is_ordered_idempotent_and_resumable() {
    use crate::execution::contract::{
        EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
    };

    let root = std::env::temp_dir().join(format!(
        "koolade-execution-journal-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let event = ExecutionEventV1 {
        schema_version: 1,
        execution_id: execution_id.clone(),
        sequence: 1,
        timestamp_unix_ms: 10,
        event: ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Queued,
        },
    };
    append_event(&task_id, &event).unwrap();
    append_event(&task_id, &event).unwrap();
    assert_eq!(
        load_events(
            &task_id,
            &execution_id,
            EventPageRequestV1 {
                after_sequence: 0,
                max_events: 256,
            },
        )
        .unwrap(),
        vec![event.clone()]
    );
    assert!(
        load_events(
            &task_id,
            &execution_id,
            EventPageRequestV1 {
                after_sequence: 1,
                max_events: 256,
            },
        )
        .unwrap()
        .is_empty()
    );

    let mut conflicting = event;
    conflicting.timestamp_unix_ms += 1;
    assert!(append_event(&task_id, &conflicting).is_err());
}

#[test]
fn execution_record_rejects_path_traversal_and_noncanonical_ids() {
    let mut record = record();
    record.artifacts.push(ExecutionArtifactV1 {
        relative_path: "../outside".into(),
        sha256: format!("sha256:{}", "c".repeat(64)),
        size_bytes: 1,
    });
    assert!(record.validate().is_err());
    assert!(load("../task", "../execution").is_err());
}

#[test]
fn execution_record_rejects_duplicate_artifact_paths() {
    let mut record = record();
    let artifact = ExecutionArtifactV1 {
        relative_path: "reports/result.json".into(),
        sha256: format!("sha256:{}", "c".repeat(64)),
        size_bytes: 1,
    };
    record.artifacts = vec![artifact.clone(), artifact];

    assert!(record.validate().is_err());
}

#[test]
fn execution_workspace_baseline_is_bounded_unique_and_path_safe() {
    let mut record = record();
    record.workspace_manifest = Some(vec![
        ExecutionWorkspaceEntryV1 {
            relative_path: "src".into(),
            kind: ExecutionWorkspaceEntryKindV1::Directory,
            sha256: None,
            size_bytes: 0,
            mode: 0o755,
        },
        ExecutionWorkspaceEntryV1 {
            relative_path: "src/main.rs".into(),
            kind: ExecutionWorkspaceEntryKindV1::File,
            sha256: Some("c".repeat(64)),
            size_bytes: 12,
            mode: 0o644,
        },
    ]);
    assert!(record.validate().is_ok());
    record.workspace_manifest.as_mut().unwrap()[1].relative_path = "../outside".into();
    assert!(record.validate().is_err());
}
