use std::{
    collections::HashSet,
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
    scenarios: &[crate::runtime_qa::QaScenarioV1],
    reviewer_run: &str,
    reviewer_session: &str,
) -> Result<PromptManifest, String> {
    let scenario_set_sha = crate::runtime_qa::scenario_set_sha256(scenarios)?;
    Ok(render(&role_input(
        request,
        reviewer_run,
        reviewer_session,
        &request.parent_task_uid,
        "evidence is produced after runtime execution".into(),
        scenario_set_sha,
        Vec::new(),
    ))?
    .manifest)
}

pub(super) struct ReviewInputs<'a> {
    pub harness: &'a dyn AiHarness,
    pub request: &'a QaRunRequestV1,
    pub scenarios: &'a [crate::runtime_qa::QaScenarioV1],
    pub evidence: &'a QaEvidenceManifestV1,
    pub candidate: &'a QaResultV1,
    pub screenshots: &'a [crate::runtime_qa::QaScreenshotArtifactV1],
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
        scenarios,
        evidence,
        candidate,
        screenshots,
        workspace,
        task_clone,
        reviewer_run,
        reviewer_session,
        cancelled,
    } = inputs;
    let subject_sha = crate::runtime_qa::qa_review_subject_sha256(request, candidate, evidence)?;
    let content = serde_json::to_string(&(request, scenarios, candidate, evidence))
        .map_err(|error| format!("runtime QA evidence prompt could not be encoded: {error}"))?;
    let screenshot_guidance = if evidence.screenshots.is_empty() {
        "This headless service run has no screenshots. Assess only the recorded functional action trace and assertions; do not make visual claims or request screenshots."
    } else {
        "Call koolade_runtime_qa_screenshot for each screenshot you use to assess visual or functional appearance; the tool returns only validated images from this run and records the IDs inspected."
    };
    let screenshot_review =
        screenshot_review(request, evidence, screenshots, &workspace.project_id)?;
    let rendered = render(&role_input(
        request,
        reviewer_run,
        reviewer_session,
        &workspace.project_id,
        content,
        subject_sha.clone(),
        vec![EvidenceItem {
            id: format!("runtime-qa-{}", request.qa_run_id),
            workspace_uid: workspace.project_id.clone(),
            origin: EvidenceOrigin::Tool,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("runtime-qa://{}", request.qa_run_id),
            content: format!(
                "The following serialized payload contains the immutable request, exact approved scenarios and their visual expectations, observed actions, screenshot metadata and storage references, assertions, and candidate result. {screenshot_guidance}"
            ),
        }],
    ))?;
    let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
    let identity = ReviewExecutionIdentity {
        execution_id: reviewer_run.to_owned(),
        session_id: reviewer_session.to_owned(),
        runtime_qa_screenshot_review: Some(Arc::clone(&screenshot_review)),
    };
    let plan = PlanningRequest {
        mode: ExecutionMode::RuntimeQaVisualReview,
        task_id: Some(request.parent_task_uid.clone()),
        // The independent model review receives the frozen QA evidence in its
        // prompt and runs without repository tools. Do not attach the browser
        // QA Docker workspace to the provider invocation.
        execution_workspace: None,
        destination_sha: None,
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
    let evidence_ids = reviewer_evidence_ids(&evidence.screenshots, &evidence.actions);
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: Some(&subject_sha),
        evidence_ids,
        required_requirements: request.requirement_ids.clone(),
        ..Default::default()
    };
    let mut verdict = match validate_output_with_context(
        PromptRole::RuntimeQaReviewerV3,
        &output.final_text,
        &context,
    )? {
        crate::harness::prompt_catalog::RoleOutputV1::Review(verdict) => verdict,
        _ => return Err("runtime QA reviewer returned the wrong output schema".into()),
    };
    verdict.tested_evidence_ids = screenshot_review
        .inspected_screenshot_ids
        .lock()
        .map_err(|_| "runtime QA screenshot inspection audit is unavailable".to_owned())?
        .iter()
        .cloned()
        .collect();
    Ok(verdict)
}

fn screenshot_review(
    request: &QaRunRequestV1,
    evidence: &QaEvidenceManifestV1,
    screenshots: &[crate::runtime_qa::QaScreenshotArtifactV1],
    workspace_id: &str,
) -> Result<Arc<crate::harness::RuntimeQaScreenshotReview>, String> {
    if screenshots.len() != evidence.screenshots.len() {
        return Err("runtime QA reviewer image set does not match the evidence manifest".into());
    }
    if screenshots.is_empty() && evidence.platform != crate::runtime_qa::QaPlatform::HeadlessService
    {
        return Err("only headless service review may omit screenshots".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut images = Vec::with_capacity(screenshots.len());
    for artifact in screenshots {
        if !ids.insert(artifact.id.as_str()) {
            return Err("runtime QA reviewer image set repeats a screenshot".into());
        }
        let metadata = evidence
            .screenshots
            .iter()
            .find(|metadata| metadata.id == artifact.id)
            .ok_or_else(|| {
                "runtime QA reviewer image has no evidence manifest record".to_owned()
            })?;
        crate::runtime_qa::validate_screenshot_artifact(
            metadata,
            &artifact.bytes,
            request.bounded_limits.max_evidence_bytes,
        )?;
        images.push(crate::harness::RuntimeQaReviewScreenshot {
            id: artifact.id.clone(),
            sha256: metadata.sha256.clone(),
            bytes: artifact.bytes.clone(),
        });
    }
    Ok(Arc::new(crate::harness::RuntimeQaScreenshotReview {
        workspace_id: workspace_id.to_owned(),
        source_sha: request.code_head_sha.clone(),
        screenshots: images,
        inspected_screenshot_ids: Arc::default(),
    }))
}

fn reviewer_evidence_ids(
    screenshots: &[crate::runtime_qa::QaScreenshotV1],
    actions: &[crate::runtime_qa::QaActionEvidenceV1],
) -> HashSet<String> {
    screenshots
        .iter()
        .map(|screenshot| screenshot.id.clone())
        .chain(actions.iter().map(|action| action.index.to_string()))
        .chain(
            actions
                .iter()
                .flat_map(|action| action.evidence_ids.iter().cloned()),
        )
        .collect()
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
        role: PromptRole::RuntimeQaReviewerV3,
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
        output_schema_version: PromptRole::RuntimeQaReviewerV3
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
