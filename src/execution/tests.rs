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
        controller_target_id: None,
        workspace: WorkspaceRef {
            project_id,
            task_id,
            source_sha: "a".repeat(40),
        },
        mode: ExecutionModeV1::Implementation,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        argv: vec!["git".into(), "status".into(), "--short".into()],
        working_directory_relative: ".".into(),
        stdin: None,
        environment_allowlist: BTreeSet::from(["CI".into()]),
        resource_budget: budget(1024 * 1024 * 1024, 2000),
        deadline_unix_ms: 10_000,
        allowed_service_ports: BTreeSet::new(),
        broker_capability_ids: BTreeSet::from([uuid::Uuid::new_v4().hyphenated().to_string()]),
        dependency_cache_refs: Vec::new(),
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

#[path = "tests/wire_contract.rs"]
mod wire_contract;

#[test]
fn request_deadline_cannot_extend_beyond_the_bounded_execution_lease() {
    let now = 1_000;
    let mut request = request();
    request.deadline_unix_ms = now + super::contract::MAX_EXECUTION_DURATION_MS;
    request.validate_at(now).unwrap();

    request.deadline_unix_ms += 1;
    assert!(request.validate_at(now).is_err());
}

#[test]
fn verification_policy_allows_the_python_and_go_stack_recipes() {
    let policy = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(policy.command_capabilities().contains("python"));
    assert!(policy.command_capabilities().contains("go"));
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
fn scoped_write_grant_is_typed_relative_and_default_denied() {
    let valid = ExecutionPolicyV1::for_scoped_workspace_write(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        "apps/web".into(),
    )
    .unwrap();
    assert_eq!(
        valid.workspace_write_scope().unwrap().root_relative,
        "apps/web"
    );
    assert!(valid.validate().is_ok());
    let project_root = ExecutionPolicyV1::for_scoped_workspace_write(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        ".".into(),
    )
    .unwrap();
    assert_eq!(
        project_root.workspace_write_scope().unwrap().root_relative,
        "."
    );
    assert!(project_root.validate().is_ok());
    for invalid in ["", "../outside", "apps/../outside", "/tmp/outside"] {
        assert!(
            ExecutionPolicyV1::for_scoped_workspace_write(
                ExecutionModeV1::Verification,
                budget(1024 * 1024 * 1024, 2000),
                invalid.into(),
            )
            .is_err(),
            "unexpectedly accepted {invalid:?}"
        );
    }
    assert!(
        ExecutionPolicyV1::for_mode(
            ExecutionModeV1::Verification,
            budget(1024 * 1024 * 1024, 2000),
            BTreeSet::new(),
        )
        .unwrap()
        .workspace_write_scope()
        .is_none()
    );
}

#[test]
fn approved_recipe_request_binds_exact_command_arguments_and_target_scope() {
    let mut request = request();
    request.mode = ExecutionModeV1::Verification;
    request.working_directory_relative = ".".into();
    request.argv = vec!["odin".into(), "build".into(), "src".into()];
    request.environment_allowlist.clear();
    request.policy = ExecutionPolicyV1::for_approved_recipe(
        budget(1024 * 1024 * 1024, 2000),
        "apps/odin".into(),
        "user-approval:decision-1".into(),
        "odin".into(),
        vec!["build".into(), "src".into()],
        ".".into(),
        BTreeSet::new(),
    )
    .unwrap()
    .with_artifact_paths(vec!["dist/index.html".into()])
    .unwrap();
    assert_eq!(request.policy.artifact_paths(), &["dist/index.html"]);
    request.validate_at(1).unwrap();

    let ceiling = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Verification,
        budget(2 * 1024 * 1024 * 1024, 4000),
        BTreeSet::new(),
    )
    .unwrap();
    request.validate_under_policy(1, &ceiling).unwrap();

    request.argv[2] = "../sibling".into();
    assert!(request.validate_at(1).is_err());
}

#[test]
fn artifact_collection_policy_rejects_unbounded_or_unsafe_paths() {
    let base = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(
        base.clone()
            .with_artifact_paths(vec!["dist/../secrets".into()])
            .is_err()
    );
    assert!(
        base.clone()
            .with_artifact_paths(vec!["dist/index.html".into(), "dist/index.html".into()])
            .is_err()
    );
    assert!(
        base.with_artifact_paths((0..129).map(|index| format!("dist/{index}.bin")).collect())
            .is_err()
    );
    assert!(
        ExecutionPolicyV1::for_mode(
            ExecutionModeV1::RuntimeQa,
            budget(1024 * 1024 * 1024, 2000),
            BTreeSet::new(),
        )
        .unwrap()
        .with_artifact_paths(vec!["dist/index.html".into()])
        .is_ok()
    );
    assert!(
        ExecutionPolicyV1::for_mode(
            ExecutionModeV1::PlanningReadOnly,
            budget(1024 * 1024 * 1024, 2000),
            BTreeSet::new(),
        )
        .unwrap()
        .with_artifact_paths(vec!["dist/index.html".into()])
        .is_err()
    );
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
fn role_selection_maps_to_security_modes_without_provider_authority() {
    assert_eq!(
        HarnessRole::Implementer.execution_mode(),
        ExecutionModeV1::Implementation
    );
    assert_eq!(
        HarnessRole::RuntimeQaReviewer.execution_mode(),
        ExecutionModeV1::RuntimeQa
    );
    assert_eq!(
        HarnessRole::RuntimeQaReviewer.harness_mode(),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
    for role in [
        HarnessRole::Manager,
        HarnessRole::PlanningAnalyst,
        HarnessRole::PlanReviewer,
        HarnessRole::TaskDecomposer,
        HarnessRole::CodeReviewer,
        HarnessRole::SpecReconciler,
    ] {
        assert_eq!(role.execution_mode(), ExecutionModeV1::PlanningReadOnly);
    }
    assert_eq!(
        HarnessRole::PRPreparer.execution_mode(),
        ExecutionModeV1::PlanningReadOnly
    );
    assert_eq!(
        HarnessRole::PRPreparer.harness_mode(),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
    assert!(!HarnessRole::PRPreparer.requires_execution_target());
    assert_eq!(
        HarnessRole::RepairWorker.execution_mode(),
        ExecutionModeV1::Implementation
    );
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

#[path = "tests/request_edges.rs"]
mod request_edges;

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
