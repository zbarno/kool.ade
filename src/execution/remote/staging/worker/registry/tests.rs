use super::*;

#[test]
fn generation_advancement_waits_for_durable_cleanup_after_worker_exit() {
    let root = tempfile::tempdir().unwrap();
    let stager = WorkerStager::new(root.path(), "fixture".into()).unwrap();
    let workspace = WorkspaceRef {
        project_id: uuid::Uuid::new_v4().to_string(),
        task_id: uuid::Uuid::new_v4().to_string(),
        source_sha: "a".repeat(40),
    };
    let directory = root.path().join("executions").join(&workspace.task_id);
    fs::create_dir_all(&directory).unwrap();
    let execution_id = uuid::Uuid::new_v4().to_string();
    let path = directory.join(format!("{execution_id}.json"));
    let mut record = serde_json::json!({
        "schema_version": 1, "execution_id": execution_id,
        "project_id": workspace.project_id, "task_id": workspace.task_id,
        "target_id": "local", "owner_instance_id": uuid::Uuid::new_v4().to_string(),
        "container_name": "koolade-exec-fixture", "workspace_source_sha": workspace.source_sha,
        "destination_sha": null, "container_id": null,
        "image_digest": format!("sha256:{}", "b".repeat(64)), "policy_version": "docker-sandbox-v1",
        "lease_generation": 1, "lease_expires_at_unix_ms": 2000, "attempt": 1,
        "state": "preparing", "event_cursor": 0, "started_at_unix_ms": 10,
        "updated_at_unix_ms": 10, "ended_at_unix_ms": null, "exit_code": null,
        "artifacts": [], "cleanup": "pending"
    });
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let error = stager
        .ensure_no_unfinished_execution(&workspace)
        .unwrap_err();
    assert!(error.to_string().contains("unfinished durable execution"));
    record["state"] = "interrupted".into();
    record["ended_at_unix_ms"] = 10.into();
    record["cleanup"] = "complete".into();
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    stager.ensure_no_unfinished_execution(&workspace).unwrap();
    fs::write(&path, b"invalid execution record").unwrap();
    assert!(stager.ensure_no_unfinished_execution(&workspace).is_err());
}
