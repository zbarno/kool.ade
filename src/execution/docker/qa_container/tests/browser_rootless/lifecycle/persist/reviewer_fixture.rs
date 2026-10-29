use crate::harness::{
    AiHarness, ExecutionMode, HarnessOutcome, PlanningRequest, ReviewExecutionIdentity,
    prompt_catalog::{
        CoverageStatus, CoverageV1, PromptRole, ReviewOutcomeV1, ReviewVerdictV1,
        validate_prompt_manifest,
    },
};

pub(super) struct RuntimeQaReviewHarness {
    subject_hash: String,
    qa_run_id: String,
    task_id: String,
    code_sha: String,
    approved_spec_hash: String,
    author_run_id: String,
    author_session_id: String,
    requirements: Vec<String>,
    evidence_ids: Vec<String>,
    screenshot_ids: Vec<String>,
    pass: bool,
    invocations: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl RuntimeQaReviewHarness {
    pub(super) fn new(
        request: &crate::runtime_qa::QaRunRequestV1,
        candidate: &crate::runtime_qa::QaResultV1,
        evidence: &crate::runtime_qa::QaEvidenceManifestV1,
    ) -> (Self, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let invocations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let evidence_ids = evidence
            .screenshots
            .iter()
            .map(|screenshot| screenshot.id.clone())
            .chain(
                evidence
                    .actions
                    .iter()
                    .map(|action| action.index.to_string()),
            )
            .collect();
        let screenshot_ids = evidence
            .screenshots
            .iter()
            .map(|screenshot| screenshot.id.clone())
            .collect();
        (
            Self {
                subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                    request, candidate, evidence,
                )
                .expect("the rootless runtime review subject is hashable"),
                qa_run_id: request.qa_run_id.clone(),
                task_id: request.parent_task_uid.clone(),
                code_sha: request.code_head_sha.clone(),
                approved_spec_hash: request.approved_spec_hash.clone(),
                author_run_id: request.author_run_id.clone(),
                author_session_id: request.author_session_id.clone(),
                requirements: request.requirement_ids.clone(),
                evidence_ids,
                screenshot_ids,
                pass: candidate.outcome == crate::runtime_qa::QaOutcome::Pass
                    && evidence.assertions.iter().all(|assertion| {
                        assertion.status == crate::runtime_qa::QaAssertionStatus::Passed
                    })
                    && evidence.browser_console.is_empty()
                    && evidence.failed_requests.is_empty(),
                invocations: std::sync::Arc::clone(&invocations),
            },
            invocations,
        )
    }

    fn assert_fresh_read_only_review(
        &self,
        request: &PlanningRequest,
        identity: &ReviewExecutionIdentity,
    ) {
        assert_eq!(request.mode, ExecutionMode::RuntimeQaVisualReview);
        assert_eq!(
            request.mode.tool_access(),
            crate::harness::ToolAccess::RuntimeQaScreenshots
        );
        assert!(request.execution_workspace.is_none());
        assert!(request.authorization_verifier.is_none());
        assert_eq!(
            request.telemetry_phase.as_deref(),
            Some("runtime_qa_review")
        );
        let role_manifest = request
            .prompt_manifest
            .as_ref()
            .expect("reviewer receives the app-owned role manifest");
        assert_eq!(
            validate_prompt_manifest(role_manifest).unwrap(),
            PromptRole::RuntimeQaReviewerV3
        );
        assert_eq!(
            role_manifest.privilege_profile,
            "read_only; isolated_fresh_review_context"
        );
        for exact_binding in [
            self.qa_run_id.as_str(),
            self.task_id.as_str(),
            self.code_sha.as_str(),
            self.approved_spec_hash.as_str(),
            self.author_run_id.as_str(),
            self.author_session_id.as_str(),
        ] {
            assert!(request.prompt_body.contains(exact_binding));
        }
        let payload = identity
            .runtime_qa_screenshot_review
            .as_ref()
            .expect("reviewer receives only this run's screenshot capability");
        assert_ne!(identity.execution_id, self.author_run_id);
        assert_ne!(identity.session_id, self.author_session_id);
        assert_eq!(
            request
                .review_identity
                .as_ref()
                .map(|review| &review.execution_id),
            Some(&identity.execution_id)
        );
        assert_eq!(
            request
                .review_identity
                .as_ref()
                .map(|review| &review.session_id),
            Some(&identity.session_id)
        );
        assert_eq!(request.task_id.as_deref(), Some(self.task_id.as_str()));
        assert_eq!(payload.source_sha, self.code_sha);
        assert_eq!(
            payload
                .screenshots
                .iter()
                .map(|screenshot| screenshot.id.clone())
                .collect::<Vec<_>>(),
            self.screenshot_ids
        );
    }

    fn verdict(&self) -> String {
        let outcome = if self.pass {
            ReviewOutcomeV1::Pass
        } else {
            ReviewOutcomeV1::BlockingFindings
        };
        serde_json::to_string(&ReviewVerdictV1 {
            subject_hash: self.subject_hash.clone(),
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: self.pass,
            outcome: Some(outcome),
            coverage: self
                .requirements
                .iter()
                .map(|requirement| CoverageV1 {
                    requirement: requirement.clone(),
                    status: if self.pass {
                        CoverageStatus::Pass
                    } else {
                        CoverageStatus::Fail
                    },
                    evidence: self.evidence_ids.clone(),
                })
                .collect(),
            findings: Vec::new(),
            untested: Vec::new(),
        })
        .expect("fixture review verdict serializes")
    }
}

impl AiHarness for RuntimeQaReviewHarness {
    fn label(&self) -> String {
        "deterministic Runtime QA reviewer fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture reviewer is available".into())
    }

    fn execute(
        &self,
        _request: &PlanningRequest,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        panic!("Runtime QA must use the independent review entrypoint")
    }

    fn execute_review(
        &self,
        request: &PlanningRequest,
        identity: &ReviewExecutionIdentity,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        self.assert_fresh_read_only_review(request, identity);
        let image_review = identity.runtime_qa_screenshot_review.as_ref().unwrap();
        let actual_images = image_review
            .screenshots
            .iter()
            .map(|screenshot| {
                assert_eq!(
                    format!("{:x}", sha2::Sha256::digest(&screenshot.bytes)),
                    screenshot.sha256
                );
                let decoded = image::load_from_memory(&screenshot.bytes)
                    .expect("reviewer fixture can decode the real screenshot pixels");
                assert!(decoded.width() > 0 && decoded.height() > 0);
                screenshot.id.clone()
            })
            .collect::<Vec<_>>();
        image_review
            .inspected_screenshot_ids
            .lock()
            .expect("screenshot inspection audit lock is not poisoned")
            .extend(actual_images);
        self.invocations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(HarnessOutcome {
            final_text: self.verdict(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

use sha2::Digest as _;
