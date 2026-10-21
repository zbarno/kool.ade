use sha2::Digest as _;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::{
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    harness::prompt_catalog::{
        CoverageStatus, EvidenceConfidentiality, EvidenceItem, EvidenceOrigin,
        OutputValidationContext, PromptRole, RoleInputV1, RoleOutputV1, RolePayloadV1, render,
        validate_output_with_context,
    },
    harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest},
};

pub(crate) struct ApplicabilityReviewHandle {
    result: mpsc::Receiver<Result<crate::runtime_qa::QaApplicabilityProposalV1, String>>,
    cancel: Arc<AtomicBool>,
}

impl ApplicabilityReviewHandle {
    pub(crate) fn start(
        assessment: crate::runtime_qa::QaApplicabilityAssessmentV1,
        task_revision: u64,
        workspace_uid: String,
        feature_uid: String,
        review_context: String,
        repo_root: std::path::PathBuf,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
        let (done_tx, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let outcome = review(
                assessment,
                task_revision,
                workspace_uid,
                feature_uid,
                review_context,
                repo_root,
                harness,
                progress_tx,
                worker_cancel,
            );
            let _ = done_tx.send(outcome);
        });
        Self { result, cancel }
    }

    pub(crate) fn take_result(
        &self,
    ) -> Option<Result<crate::runtime_qa::QaApplicabilityProposalV1, String>> {
        self.result.try_recv().ok()
    }
}

impl Drop for ApplicabilityReviewHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

#[allow(clippy::too_many_arguments)]
fn review(
    assessment: crate::runtime_qa::QaApplicabilityAssessmentV1,
    task_revision: u64,
    workspace_uid: String,
    feature_uid: String,
    review_context: String,
    repo_root: std::path::PathBuf,
    harness: Box<dyn AiHarness>,
    progress_tx: mpsc::Sender<LiveProgress>,
    cancel: Arc<AtomicBool>,
) -> Result<crate::runtime_qa::QaApplicabilityProposalV1, String> {
    assessment.validate()?;
    if review_context.trim().is_empty() || review_context.len() > 128 * 1024 {
        return Err("headless applicability review context is missing or too large".into());
    }
    if cancel.load(Ordering::Acquire) {
        return Err("runtime QA applicability review was cancelled".into());
    }
    let encoded = serde_json::to_vec(&assessment).map_err(|error| error.to_string())?;
    let assessment_hash = format!("{:x}", sha2::Sha256::digest(encoded));
    let evidence_id = format!("headless-target:{}", assessment.target_id);
    let reviewer_identity = crate::harness::ReviewExecutionIdentity {
        execution_id: assessment.reviewer_execution_id.clone(),
        session_id: assessment.reviewer_session_id.clone(),
    };
    let input = RoleInputV1 {
        role: PromptRole::RuntimeQaReviewerV2,
        workspace_uid: workspace_uid.clone(),
        feature_uid,
        task_uid: Some(assessment.task_uid.clone()),
        original_goal: "Independently verify that visual runtime QA does not apply to the selected headless service target.".into(),
        approved_spec_ref: Some(format!("approved-spec:{}", assessment.approved_spec_hash)),
        approved_spec_hash: Some(assessment.approved_spec_hash.clone()),
        decision_refs: vec!["VISUAL_QA_APPLICABILITY".into()],
        evidence_manifest: vec![EvidenceItem {
            id: evidence_id.clone(),
            workspace_uid,
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("project-runtime://target/{}", assessment.target_id),
            content: format!(
                "Assessment:\n{}\n\nPersisted task context:\n{}",
                serde_json::to_string(&assessment).map_err(|error| error.to_string())?,
                review_context
            ),
        }],
        execution_policy_ref: "runtime-qa-applicability/read-only-v1".into(),
        output_schema_version: PromptRole::RuntimeQaReviewerV2.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: assessment.code_sha.clone(),
            subject_hash: assessment_hash.clone(),
            author_run_id: assessment.approved_by.clone(),
            reviewer_run_id: assessment.reviewer_execution_id.clone(),
            author_session_id: Some(assessment.manager_session_id.clone()),
            reviewer_session_id: Some(assessment.reviewer_session_id.clone()),
            requirements_matrix: vec![
                "VISUAL_QA_APPLICABILITY: confirm the exact selected target is a headless service and visual QA is not being represented as passed".into(),
            ],
            explicit_exclusions: vec![
                "This assessment does not certify API, service, or integration test coverage.".into(),
            ],
        },
    };
    let rendered = render(&input)?;
    let plan = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: Some(assessment.task_uid.clone()),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        prompt_manifest: Some(rendered.manifest.clone()),
        reasoning_level: "high".into(),
        telemetry_phase: Some("runtime_qa_applicability_review".into()),
        repo_root,
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: std::time::Duration::from_secs(180),
        progress_tx,
        cancel: Arc::clone(&cancel),
        review_identity: Some(reviewer_identity),
    };
    let output = harness
        .execute_review(
            &plan,
            plan.review_identity.as_ref().expect("review identity set"),
        )
        .map_err(|error| error.detail())?;
    if cancel.load(Ordering::Acquire) {
        return Err("runtime QA applicability review was cancelled".into());
    }
    let required = vec!["VISUAL_QA_APPLICABILITY".to_owned()];
    let evidence_ids = std::collections::HashSet::from([evidence_id]);
    let context = OutputValidationContext {
        expected_subject_hash: Some(&assessment_hash),
        required_requirements: required,
        evidence_ids,
        ..Default::default()
    };
    let RoleOutputV1::Review(verdict) = validate_output_with_context(
        PromptRole::RuntimeQaReviewerV2,
        &output.final_text,
        &context,
    )?
    else {
        return Err("runtime QA applicability reviewer returned an unexpected schema".into());
    };
    if !verdict.pass
        || !verdict.findings.is_empty()
        || !verdict.untested.is_empty()
        || !verdict.coverage.iter().any(|coverage| {
            coverage.requirement == "VISUAL_QA_APPLICABILITY"
                && coverage.status == CoverageStatus::Pass
        })
    {
        return Err(
            "independent reviewer did not approve the headless applicability assessment".into(),
        );
    }
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: assessment.task_uid.clone(),
        subject_sha256: assessment_hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: assessment.recorded_at_unix_ms,
        evidence_refs: vec![format!(
            "runtime-qa-applicability-assessment:{assessment_hash}"
        )],
        prompt_sha256: Some(rendered.manifest.identity_sha256()),
        prompt_manifest: Some(rendered.manifest),
        reviewer_run_id: Some(assessment.reviewer_execution_id.clone()),
        author_run_id: Some(assessment.approved_by.clone()),
        reviewer_session_id: Some(assessment.reviewer_session_id.clone()),
        author_session_id: Some(assessment.manager_session_id.clone()),
    };
    receipt.validate()?;
    let proposal = crate::runtime_qa::QaApplicabilityProposalV1 {
        schema_version: 1,
        task_revision,
        assessment,
        reviewer_verdict: verdict,
        reviewer_receipt: receipt,
    };
    proposal.validate()?;
    Ok(proposal)
}

#[cfg(test)]
#[path = "applicability_review/tests.rs"]
mod tests;
