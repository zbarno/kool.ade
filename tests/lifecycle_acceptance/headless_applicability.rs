use super::{plan, support::*};
use crate::task_review;
use koolade::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleActor, LifecycleEventKind, LifecycleStore,
        QaExecutionStatusV1, TaskEvent, TaskPhase,
    },
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
    },
    runtime_qa::{QaApplicability, QaApplicabilityAssessmentV1},
};
use sha2::Digest as _;

fn runtime_recipe(recipe_id: &str) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: vec!["-m".into(), "compileall".into(), ".".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["__pycache__".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:synthetic-fixture".into()],
    }
}

fn runtime_receipt(
    task_id: &str,
    target_id: &str,
    source_sha: &str,
    operation: RuntimeOperation,
    recipe_id: &str,
    state: RuntimeReceiptState,
    observed_at_unix_ms: u64,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: id(),
        operation,
        task_id: task_id.into(),
        target_id: target_id.into(),
        source_sha: source_sha.into(),
        recipe_id: Some(recipe_id.into()),
        state,
        exit_code: Some(if state == RuntimeReceiptState::Passed {
            0
        } else {
            1
        }),
        artifact_hashes: Default::default(),
        prepared_dependency_ids: Vec::new(),
        log_reference: None,
        observed_at_unix_ms,
    }
}

/// Production boundary: reviewed task → lifecycle applicability event → reopened task/event log.
/// Unit tests cannot prove the Manager actor, immutable event, and task projection survive store
/// restart together. Deterministic in-process fixtures only; no Docker/network/model dependency;
/// setup cost is the standard lifecycle acceptance fixture.
#[path = "headless_applicability/acceptance.rs"]
mod acceptance;
