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
        destination_sha: None,
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

#[path = "tests/policy.rs"]
mod policy;
#[path = "tests/wire_contract.rs"]
mod wire_contract;

#[path = "tests/request_validation.rs"]
mod request_validation;

#[path = "tests/request_edges.rs"]
mod request_edges;
