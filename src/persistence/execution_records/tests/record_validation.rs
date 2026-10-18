use super::*;

#[test]
fn legacy_execution_record_without_artifact_media_type_still_deserializes() {
    let legacy_record = r#"
    {
      "schema_version": 1,
      "execution_id": "00000000-0000-4000-8000-000000000001",
      "project_id": "00000000-0000-4000-8000-000000000002",
      "task_id": "00000000-0000-4000-8000-000000000003",
      "target_id": "local",
      "owner_instance_id": "00000000-0000-4000-8000-000000000004",
      "container_name": "koolade-exec-legacy",
      "workspace_source_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "container_id": null,
      "image_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      "policy_version": "docker-sandbox-v1",
      "lease_generation": 1,
      "attempt": 1,
      "state": "preparing",
      "event_cursor": 0,
      "started_at_unix_ms": 10,
      "updated_at_unix_ms": 10,
      "ended_at_unix_ms": null,
      "exit_code": null,
      "artifacts": [
        {
          "relative_path": "reports/result.json",
          "sha256": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
          "size_bytes": 3
        }
      ],
      "cleanup": "pending"
    }
    "#;

    let record: ExecutionRecordV1 = serde_json::from_str(legacy_record)
        .expect("legacy record without media_type should deserialize");
    record.validate().unwrap();
    assert_eq!(record.artifacts.len(), 1);
    assert_eq!(record.artifacts[0].media_type, None);
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
    record.destination_sha = Some("d".repeat(40));
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
        destination_sha: record.destination_sha.clone(),
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
    record.request.as_mut().unwrap().destination_sha = None;
    assert!(record.validate().is_err());
    record.request.as_mut().unwrap().destination_sha = record.destination_sha.clone();
    record.request.as_mut().unwrap().controller_target_id = None;
    assert!(record.validate().is_err());
}

#[test]
fn execution_record_rejects_path_traversal_and_noncanonical_ids() {
    let mut record = record();
    record.artifacts.push(ExecutionArtifactV1 {
        relative_path: "../outside".into(),
        sha256: format!("sha256:{}", "c".repeat(64)),
        size_bytes: 1,
        media_type: None,
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
        media_type: None,
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
