use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

use crate::{
    execution::contract::WorkspaceRef,
    harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptManifest, PromptRole,
        ReviewVerdictV1, RoleInputV1, RolePayloadV1, render, validate_output_with_context,
    },
    harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest, ReviewExecutionIdentity},
    runtime_qa::{QaEvidenceManifestV1, QaResultV1, QaRunRequestV1},
};

pub(super) fn manifest(
    request: &QaRunRequestV1,
    reviewer_run: &str,
    reviewer_session: &str,
) -> Result<PromptManifest, String> {
    Ok(render(&role_input(
        request,
        reviewer_run,
        reviewer_session,
        &request.parent_task_uid,
        "evidence is produced after runtime execution".into(),
        request.review_subject_sha256.clone(),
        Vec::new(),
    ))?
    .manifest)
}

pub(super) struct ReviewInputs<'a> {
    pub harness: &'a dyn AiHarness,
    pub request: &'a QaRunRequestV1,
    pub evidence: &'a QaEvidenceManifestV1,
    pub candidate: &'a QaResultV1,
    pub workspace: &'a WorkspaceRef,
    pub task_clone: &'a Path,
    pub reviewer_run: &'a str,
    pub reviewer_session: &'a str,
    pub cancelled: Arc<AtomicBool>,
}

pub(super) fn execute(inputs: ReviewInputs<'_>) -> Result<ReviewVerdictV1, String> {
    let ReviewInputs {
        harness,
        request,
        evidence,
        candidate,
        workspace,
        task_clone,
        reviewer_run,
        reviewer_session,
        cancelled,
    } = inputs;
    let evidence_sha = evidence.sha256()?;
    let content = serde_json::to_string(&(request, candidate, evidence))
        .map_err(|error| format!("runtime QA evidence prompt could not be encoded: {error}"))?;
    let rendered = render(&role_input(
        request,
        reviewer_run,
        reviewer_session,
        &workspace.project_id,
        content,
        evidence_sha.clone(),
        vec![EvidenceItem {
            id: format!("runtime-qa-{}", request.qa_run_id),
            workspace_uid: workspace.project_id.clone(),
            origin: EvidenceOrigin::Tool,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("runtime-qa://{}", request.qa_run_id),
            content: "The following serialized payload contains the immutable request, observed actions, screenshot metadata and storage references, assertions, and candidate result. Screenshot image bytes are not included.".into(),
        }],
    ))?;
    let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
    let identity = ReviewExecutionIdentity {
        execution_id: reviewer_run.to_owned(),
        session_id: reviewer_session.to_owned(),
    };
    let plan = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: Some(request.parent_task_uid.clone()),
        // The independent model review receives the frozen QA evidence in its
        // prompt and runs without repository tools. Do not attach the browser
        // QA Docker workspace to the provider invocation.
        execution_workspace: None,
        authorization_verifier: None,
        prompt_manifest: Some(rendered.manifest),
        reasoning_level: "high".into(),
        telemetry_phase: Some("runtime_qa_review".into()),
        repo_root: task_clone.to_path_buf(),
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: Duration::from_secs(180),
        progress_tx,
        cancel: Arc::clone(&cancelled),
        review_identity: Some(identity.clone()),
    };
    let output = harness
        .execute_review(&plan, &identity)
        .map_err(|error| error.detail())?;
    let evidence_ids = evidence
        .screenshots
        .iter()
        .map(|shot| shot.id.clone())
        .chain(
            evidence
                .actions
                .iter()
                .map(|action| action.index.to_string()),
        )
        .collect();
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: Some(&evidence_sha),
        evidence_ids,
        required_requirements: request.requirement_ids.clone(),
        ..Default::default()
    };
    match validate_output_with_context(
        PromptRole::RuntimeQaReviewerV2,
        &output.final_text,
        &context,
    )? {
        crate::harness::prompt_catalog::RoleOutputV1::Review(verdict) => Ok(verdict),
        _ => Err("runtime QA reviewer returned the wrong output schema".into()),
    }
}

fn role_input(
    request: &QaRunRequestV1,
    reviewer_run: &str,
    reviewer_session: &str,
    workspace_uid: &str,
    content: String,
    subject_hash: String,
    evidence_manifest: Vec<EvidenceItem>,
) -> RoleInputV1 {
    RoleInputV1 {
        role: PromptRole::RuntimeQaReviewerV2,
        workspace_uid: workspace_uid.to_owned(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: request.acceptance_criteria.join("\n"),
        approved_spec_ref: None,
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: evidence_manifest
            .into_iter()
            .map(|mut item| {
                item.content = content.clone();
                item
            })
            .collect(),
        execution_policy_ref: "runtime-qa-review/read-only-v1".into(),
        output_schema_version: PromptRole::RuntimeQaReviewerV2
            .output_schema_version()
            .into(),
        payload: RolePayloadV1::Review {
            subject_revision: request.qa_run_id.clone(),
            subject_hash,
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: reviewer_run.into(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session.into()),
            requirements_matrix: request
                .requirement_ids
                .iter()
                .zip(&request.acceptance_criteria)
                .map(|(id, criterion)| format!("{id}: {criterion}"))
                .collect(),
            explicit_exclusions: Vec::new(),
        },
    }
}

#[cfg(test)]
#[path = "reviewer/tests.rs"]
mod tests;
