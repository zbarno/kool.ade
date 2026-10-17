use super::{Implementation, Runner};
use crate::harness::{ExecutionMode, PlanningRequest};

pub(super) fn build(
    state: &Implementation,
    runner: &Runner,
    prompt: String,
    attempt: usize,
    verification_corrections: usize,
) -> anyhow::Result<PlanningRequest> {
    use crate::harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, RoleInputV1,
        RolePayloadV1,
    };
    use sha2::Digest;

    let role = if verification_corrections > 0 || attempt > 1 {
        PromptRole::RepairWorker
    } else {
        PromptRole::Implementer
    };
    let workspace_uid = state
        .workspace_uid
        .clone()
        .or_else(|| state.project_id.clone())
        .or_else(|| state.task_uid.clone())
        .unwrap_or_else(|| state.ticket.clone());
    let approved_spec_hash = state
        .approved_specification
        .as_ref()
        .map(|spec| format!("{:x}", sha2::Sha256::digest(spec.as_bytes())));
    let allowed_execution_tools = vec![
        "koolade_bash".into(),
        "koolade_resource".into(),
        "koolade_dependency".into(),
    ];
    let payload = match role {
        PromptRole::RepairWorker => RolePayloadV1::Repair {
            assigned_blocking_findings: vec![
                "Resolve only the blocking report or verification findings in the supplied task evidence.".into(),
            ],
            approved_task_baseline: state.base_commit.clone(),
            allowed_execution_tools,
        },
        _ => RolePayloadV1::Worker {
            assigned_task: state.ticket_text.clone(),
            assigned_subtasks: Vec::new(),
            immutable_baseline: state.base_commit.clone(),
            allowed_execution_tools,
        },
    };
    let prompt_input = RoleInputV1 {
        role,
        workspace_uid: workspace_uid.clone(),
        feature_uid: state
            .task_uid
            .clone()
            .unwrap_or_else(|| state.ticket.clone()),
        task_uid: state.task_uid.clone(),
        original_goal: crate::core::context_build::clip(&state.ticket_text, 4000),
        approved_spec_ref: state
            .approved_specification
            .as_ref()
            .map(|_| "approved-task-specification".into()),
        approved_spec_hash,
        decision_refs: Vec::new(),
        evidence_manifest: vec![EvidenceItem {
            id: "implementation-request".into(),
            workspace_uid,
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "application-built implementation task context".into(),
            content: prompt,
        }],
        execution_policy_ref: "implementation/task-clone/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload,
    };
    let rendered =
        crate::harness::prompt_catalog::render(&prompt_input).map_err(anyhow::Error::msg)?;
    Ok(PlanningRequest {
        prompt_manifest: Some(rendered.manifest),
        mode: ExecutionMode::Implementation,
        task_id: state
            .task_uid
            .clone()
            .or_else(|| Some(state.ticket.clone())),
        execution_workspace: state
            .workspace_uid
            .as_ref()
            .zip(state.task_uid.as_ref())
            .map(
                |(project_id, task_id)| crate::execution::contract::WorkspaceRef {
                    project_id: project_id.clone(),
                    task_id: task_id.clone(),
                    source_sha: state.base_commit.clone(),
                },
            ),
        destination_sha: runner.destination_sha_for(state)?,
        reasoning_level: "medium".into(),
        telemetry_phase: Some(
            if verification_corrections > 0 {
                "qa_verification"
            } else if attempt == 1 {
                "implementation"
            } else {
                "repair"
            }
            .into(),
        ),
        repo_root: state.task_repository.clone(),
        runtime_config_source: runner.runtime_config_source.clone(),
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: runner.remaining()?,
        progress_tx: runner.progress.clone(),
        cancel: runner.cancel.clone(),
        authorization_verifier: runner.authorization_verifier.clone(),
        review_identity: None,
    })
}
