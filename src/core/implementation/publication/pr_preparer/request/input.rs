use super::super::*;
use super::CHUNK_BYTES;
use crate::core::lifecycle::{FeatureGoal, TaskRecord};
use crate::harness::prompt_catalog::{
    EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, RoleInputV1, RolePayloadV1,
};
use crate::harness::{ExecutionMode, PlanningRequest};

// Keep quoted content below the catalog's 64 KiB rendered-evidence ceiling
// while leaving room for source labels and per-chunk metadata.
const MAX_EVIDENCE_BYTES: usize = 56 * 1024;
pub(in crate::core::implementation::publication::pr_preparer) fn build_request(
    state: &Implementation,
    task: &TaskRecord,
    included: &[TaskRecord],
    feature: &FeatureGoal,
    scope: &ProposalScope,
    runner: &Runner,
) -> anyhow::Result<PromptRequest> {
    let role = PromptRole::PrPreparer;
    let workspace_uid = state
        .workspace_uid
        .clone()
        .or_else(|| state.project_id.clone())
        .unwrap_or_else(|| feature.feature_uid.clone());
    let mut evidence = Vec::new();
    let mut evidence_bytes = 0usize;
    add_chunks(
        &mut evidence,
        &mut evidence_bytes,
        "approved-feature-plan",
        "user-approved feature plan",
        EvidenceOrigin::User,
        &workspace_uid,
        &scope.feature_plan,
    )?;
    add_chunks(
        &mut evidence,
        &mut evidence_bytes,
        "approved-task-spec",
        "approved lifecycle task specification",
        EvidenceOrigin::User,
        &workspace_uid,
        &scope.task_spec,
    )?;
    add_chunks(
        &mut evidence,
        &mut evidence_bytes,
        "lifecycle-gates",
        "application-validated review, QA, and specification receipts",
        EvidenceOrigin::Application,
        &workspace_uid,
        &scope.gate_evidence,
    )?;
    add_chunks(
        &mut evidence,
        &mut evidence_bytes,
        "verified-test-report",
        "application-validated verification report",
        EvidenceOrigin::Application,
        &workspace_uid,
        &scope.verified_report,
    )?;
    add_chunks(
        &mut evidence,
        &mut evidence_bytes,
        "integrated-diff",
        "complete immutable integrated Git diff; chunks form the complete diff",
        EvidenceOrigin::Repository,
        &workspace_uid,
        &scope.diff,
    )?;
    add_chunks(
        &mut evidence,
        &mut evidence_bytes,
        "pr-scope-manifest",
        "application-derived exact publication scope",
        EvidenceOrigin::Application,
        &workspace_uid,
        &scope.scope_manifest,
    )?;
    anyhow::ensure!(
        evidence.len() <= 128
            && evidence_bytes <= MAX_EVIDENCE_BYTES
            && evidence.iter().map(|item| item.id.as_str()).eq(scope
                .expected
                .evidence_ids
                .iter()
                .map(String::as_str)),
        "PR Preparer evidence exceeds the supported request size or changed while rendering"
    );
    let reviewed_task_refs = included
        .iter()
        .map(|record| {
            format!(
                "{}:plan={}:reviewed-head={}:reviewed-subject={}",
                record.task_uid,
                record.approved_plan_sha256,
                record.reviewed_code_head_sha.as_deref().unwrap_or_default(),
                record
                    .reviewed_code_subject_sha256
                    .as_deref()
                    .unwrap_or_default()
            )
        })
        .collect();
    let approved_subtasks = included
        .iter()
        .filter(|record| record.task_uid != task.task_uid)
        .map(|record| format!("{}:plan={}", record.task_uid, record.approved_plan_sha256))
        .collect();
    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role,
        workspace_uid,
        feature_uid: feature.feature_uid.clone(),
        task_uid: Some(task.task_uid.clone()),
        original_goal: crate::core::context_build::clip(&feature.original_intent, 4000),
        approved_spec_ref: Some(
            feature
                .plan_snapshot_uid
                .clone()
                .unwrap_or_else(|| feature.feature_uid.clone()),
        ),
        approved_spec_hash: feature.approved_plan_sha256.clone(),
        decision_refs: feature.decision_refs.clone(),
        evidence_manifest: evidence,
        execution_policy_ref: "publication/pr-preparer/read-only-exact-scope/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Pr {
            reviewed_task_refs,
            approved_subtasks,
            base_ref: scope.expected.base_ref.clone(),
            head_ref: scope.expected.head_ref.clone(),
        },
    })
    .map_err(anyhow::Error::msg)?;
    let request = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: Some(task.task_uid.clone()),
        execution_workspace: None,
        authorization_verifier: None,
        prompt_manifest: Some(rendered.manifest.clone()),
        reasoning_level: "high".into(),
        telemetry_phase: Some("pr_preparation".into()),
        repo_root: state.task_repository.clone(),
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: runner.remaining()?,
        progress_tx: runner.progress.clone(),
        cancel: runner.cancel.clone(),
        review_identity: None,
    };
    Ok(PromptRequest {
        request,
        manifest: rendered.manifest,
    })
}

fn add_chunks(
    evidence: &mut Vec<EvidenceItem>,
    total_bytes: &mut usize,
    prefix: &str,
    source: &str,
    origin: EvidenceOrigin,
    workspace_uid: &str,
    content: &str,
) -> anyhow::Result<()> {
    let mut offset = 0;
    let mut index = 0;
    while offset < content.len().max(1) {
        let end = if content.is_empty() {
            0
        } else {
            let mut end = (offset + CHUNK_BYTES).min(content.len());
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            end
        };
        let chunk = if content.is_empty() {
            ""
        } else {
            &content[offset..end]
        };
        *total_bytes = total_bytes.saturating_add(chunk.len());
        anyhow::ensure!(
            *total_bytes <= MAX_EVIDENCE_BYTES,
            "PR Preparer evidence exceeds the supported request size"
        );
        evidence.push(EvidenceItem {
            id: format!("{prefix}-{index}"),
            workspace_uid: workspace_uid.to_owned(),
            origin,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: source.to_owned(),
            content: chunk.to_owned(),
        });
        index += 1;
        offset = end;
        if content.is_empty() {
            break;
        }
    }
    Ok(())
}
