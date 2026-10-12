use super::{
    contract::{
        ExecutionModeV1, ExecutionRequestV1, ExecutionTarget, ExecutionWireMessageV1, HarnessRole,
        ResourceBudgetV1, TargetLocation, WorkspaceRef, ensure_wire_compatible,
    },
    errors::ExecutionErrorKind,
    policy::ExecutionPolicyV1,
};
use std::collections::BTreeSet;

fn budget(memory: u64, cpu: u32) -> ResourceBudgetV1 {
    ResourceBudgetV1 {
        cpu_millis: cpu,
        memory_bytes: memory,
        process_count: 32,
        temporary_disk_bytes: 1024 * 1024 * 1024,
        output_bytes: 8 * 1024 * 1024,
    }
}

fn request() -> ExecutionRequestV1 {
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    ExecutionRequestV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        project_id: project_id.clone(),
        task_id: task_id.clone(),
        target: ExecutionTarget::local(),
        workspace: WorkspaceRef {
            project_id,
            task_id,
            source_sha: "a".repeat(40),
        },
        mode: ExecutionModeV1::Implementation,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        argv: vec!["git".into(), "status".into(), "--short".into()],
        environment_allowlist: BTreeSet::from(["CI".into()]),
        resource_budget: budget(1024 * 1024 * 1024, 2000),
        deadline_unix_ms: 10_000,
        allowed_service_ports: BTreeSet::new(),
        broker_capability_ids: BTreeSet::from([uuid::Uuid::new_v4().hyphenated().to_string()]),
        cancellation_generation: 1,
        request_sequence: 1,
        policy: ExecutionPolicyV1::for_mode(
            ExecutionModeV1::Implementation,
            budget(2 * 1024 * 1024 * 1024, 4000),
            BTreeSet::new(),
        )
        .unwrap(),
    }
}

#[path = "tests/events.rs"]
mod events;

#[path = "tests/readiness_results.rs"]
mod readiness_results;

#[test]
fn versioned_request_round_trips_without_controller_paths_or_environment_values() {
    let request = request();
    request.validate_at(1).unwrap();
    let message = ExecutionWireMessageV1 {
        wire_version: 1,
        request,
    };
    let bytes = serde_json::to_vec(&message).unwrap();
    let decoded: ExecutionWireMessageV1 = serde_json::from_slice(&bytes).unwrap();
    decoded.validate_at(1).unwrap();
    assert_eq!(decoded, message);
    assert!(ensure_wire_compatible(1).is_ok());
    assert_eq!(
        ensure_wire_compatible(2),
        Err(ExecutionErrorKind::WorkerIncompatible)
    );
    let json = String::from_utf8(bytes).unwrap();
    assert!(!json.contains("/home/"));
    assert!(!json.contains("github_token"));
}

#[test]
fn role_selection_maps_to_security_modes_without_provider_authority() {
    assert_eq!(
        HarnessRole::Implementation.execution_mode(),
        ExecutionModeV1::Implementation
    );
    assert_eq!(
        HarnessRole::QualityAssurance.execution_mode(),
        ExecutionModeV1::RuntimeQa
    );
    assert_eq!(
        HarnessRole::QualityAssurance.harness_mode(),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
    for role in [
        HarnessRole::Manager,
        HarnessRole::Specification,
        HarnessRole::TaskGeneration,
        HarnessRole::Investigation,
        HarnessRole::Documentation,
        HarnessRole::Reconciliation,
        HarnessRole::DecisionExplanation,
    ] {
        assert_eq!(role.execution_mode(), ExecutionModeV1::PlanningReadOnly);
    }
}

#[test]
fn policy_intersection_never_widens_capabilities_or_resource_limits() {
    let broad = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Implementation,
        budget(4 * 1024 * 1024 * 1024, 4000),
        BTreeSet::new(),
    )
    .unwrap();
    let mut narrow_json = serde_json::to_value(&broad).unwrap();
    narrow_json["command_capabilities"] = serde_json::json!(["git"]);
    narrow_json["tool_capabilities"] = serde_json::json!(["read_repository"]);
    narrow_json["filesystem"] = serde_json::json!(["workspace_read"]);
    let narrow: ExecutionPolicyV1 = serde_json::from_value(narrow_json).unwrap();
    let effective = broad.restrict_to(&narrow).unwrap();
    assert_eq!(
        effective.command_capabilities(),
        &BTreeSet::from(["git".into()])
    );
    assert_eq!(
        effective.tool_capabilities(),
        &BTreeSet::from(["read_repository".into()])
    );
    assert!(
        effective
            .filesystem_grants()
            .is_subset(broad.filesystem_grants())
    );
    assert!(
        effective
            .filesystem_grants()
            .is_subset(narrow.filesystem_grants())
    );
    assert!(effective.resource_limits().memory_bytes <= broad.resource_limits().memory_bytes);
    assert!(effective.resource_limits().memory_bytes <= narrow.resource_limits().memory_bytes);

    let mut widened_json = serde_json::to_value(&narrow).unwrap();
    widened_json["command_capabilities"] = serde_json::json!(["docker"]);
    let widened: ExecutionPolicyV1 = serde_json::from_value(widened_json).unwrap();
    assert!(widened.validate().is_err());

    let request = request();
    assert!(request.validate_under_policy(1, &broad).is_ok());
    let low_ceiling = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Implementation,
        budget(512 * 1024 * 1024, 1000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(request.validate_under_policy(1, &low_ceiling).is_err());
}

#[test]
fn request_validation_rejects_invalid_ids_host_paths_and_shell_command_strings() {
    let mut invalid_id = request();
    invalid_id.execution_id = "exec-1".into();
    assert!(invalid_id.validate_at(1).is_err());

    let mut host_path = request();
    host_path.argv.push("/home/operator-a/project".into());
    assert!(host_path.validate_at(1).is_err());

    let mut shell = request();
    shell.argv = vec!["bash".into(), "-c".into(), "git status".into()];
    assert!(shell.validate_at(1).is_err());

    let mut combined_shell = request();
    combined_shell.argv = vec!["bash".into(), "-lc".into(), "git status".into()];
    assert!(combined_shell.validate_at(1).is_err());

    let mut unicode_shell = request();
    unicode_shell.argv = vec!["bash".into(), "-éclair".into()];
    assert!(unicode_shell.validate_at(1).is_err());

    let mut secret_name = request();
    secret_name
        .environment_allowlist
        .insert("GITHUB_TOKEN".into());
    assert!(secret_name.validate_at(1).is_err());
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

#[test]
fn cancelled_and_expired_requests_are_reported_without_starting() {
    let request = request();
    assert_eq!(
        request.validate_control(2, 2, false),
        Err(ExecutionErrorKind::Cancelled)
    );
    assert_eq!(
        request.validate_control(10_000, 1, false),
        Err(ExecutionErrorKind::DeadlineExceeded)
    );
    assert!(request.validate_control(2, 1, false).is_ok());
}
