use super::*;

#[test]
fn request_deadline_cannot_extend_beyond_the_bounded_execution_lease() {
    let now = 1_000;
    let mut request = request();
    request.deadline_unix_ms = now + super::super::contract::MAX_EXECUTION_DURATION_MS;
    request.validate_at(now).unwrap();

    request.deadline_unix_ms += 1;
    assert!(request.validate_at(now).is_err());
}

#[test]
fn execution_request_accepts_only_a_valid_optional_destination_sha() {
    let now = 1_000;
    let mut request = request();
    request.destination_sha = Some("c".repeat(40));
    request.validate_at(now).unwrap();
    request.destination_sha = Some("not-a-commit".into());
    assert!(request.validate_at(now).is_err());
}

#[test]
fn request_working_directory_cannot_escape_the_task_workspace() {
    let mut request = request();
    request.working_directory_relative = "packages/web".into();
    request.validate_at(1).unwrap();
    request.working_directory_relative = "packages/../../outside".into();
    assert!(request.validate_at(1).is_err());
    request.working_directory_relative = "/etc".into();
    assert!(request.validate_at(1).is_err());
}

#[test]
fn controller_target_audit_binding_is_separate_from_worker_transport_target() {
    let mut request = request();
    let worker_id = uuid::Uuid::new_v4().hyphenated().to_string();
    request.controller_target_id = Some(worker_id.clone());
    assert_eq!(request.target, ExecutionTarget::local());
    request.validate_at(1).unwrap();

    request.controller_target_id = Some("../../untrusted".into());
    assert!(request.validate_at(1).is_err());
}

#[test]
fn target_and_qa_ports_are_separate_from_workspace_paths() {
    let worker_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let remote = ExecutionTarget::remote(worker_id.clone());
    remote.validate().unwrap();
    assert_eq!(remote.location, TargetLocation::Remote { worker_id });
    let local = ExecutionTarget::local();
    local.validate().unwrap();

    let mut qa = request();
    qa.mode = ExecutionModeV1::RuntimeQa;
    qa.argv = vec!["browser".into(), "capture".into()];
    qa.allowed_service_ports = BTreeSet::from([3000]);
    qa.policy = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::RuntimeQa,
        budget(1024 * 1024 * 1024, 1000),
        BTreeSet::from([3000]),
    )
    .unwrap();
    qa.resource_budget = budget(1024 * 1024 * 1024, 1000);
    qa.validate_at(1).unwrap();
    assert_eq!(
        serde_json::to_value(&qa.policy).unwrap()["network_access"],
        "runtime_qa_service_ports"
    );
    qa.allowed_service_ports.insert(3001);
    assert!(qa.validate_at(1).is_err());
}
