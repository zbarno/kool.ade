use super::*;

fn validated_target(request: &QaRunRequestV1) -> QaValidatedTargetV1 {
    QaValidatedTargetV1 {
        task_uid: request.parent_task_uid.clone(),
        build_artifact_sha256: request.build_artifact_sha256.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        executable: "/workspace/build/app".into(),
        working_directory: "/workspace".into(),
        arguments: vec!["--qa".into()],
    }
}

fn service_lease(request: &QaRunRequestV1) -> QaTaskServiceLeaseV1 {
    QaTaskServiceLeaseV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        task_container_id: "a".repeat(64),
        task_network_id: "b".repeat(64),
        preview_port: 4173,
        task_build_sha: request.build_artifact_sha256.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        internal_network_only: true,
        published_ports: false,
        expires_at_unix_ms: now_unix_ms() + 60_000,
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn validated_target_is_bound_to_task_build_and_recipe_without_shell_paths() {
    let request = request();
    let target = validated_target(&request);
    assert!(target.validate_for(&request).is_ok());

    let mut outside = target.clone();
    outside.executable = "/workspace/../etc/passwd".into();
    assert!(outside.validate_for(&request).is_err());

    let mut stale = target;
    stale.build_artifact_sha256 = "f".repeat(64);
    assert!(stale.validate_for(&request).is_err());
}

#[test]
fn task_service_lease_must_be_private_and_bound_to_the_exact_qa_run() {
    let request = request();
    let lease = service_lease(&request);
    assert!(lease.validate_for(&request).is_ok());

    let mut exposed = lease.clone();
    exposed.published_ports = true;
    assert!(exposed.validate_for(&request).is_err());

    let mut stale = lease;
    stale.qa_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(stale.validate_for(&request).is_err());
}
