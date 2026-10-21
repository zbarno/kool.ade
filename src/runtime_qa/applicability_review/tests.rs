use super::*;
use crate::{
    error::AppError,
    harness::{HarnessOutcome, ReviewExecutionIdentity},
};
use std::time::{Duration, Instant};

struct FakeReviewer {
    response: String,
    identity: ReviewExecutionIdentity,
}

impl AiHarness for FakeReviewer {
    fn label(&self) -> String {
        "applicability review fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("ready".into())
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        assert_eq!(request.mode, ExecutionMode::ReadOnlyAnalysis);
        assert!(request.execution_workspace.is_none());
        assert!(request.prompt_manifest.as_ref().is_some_and(|manifest| {
            manifest.role_version == PromptRole::RuntimeQaReviewerV2.id()
        }));
        assert_eq!(request.review_identity.as_ref(), Some(&self.identity));
        Ok(HarnessOutcome {
            final_text: self.response.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn assessment() -> crate::runtime_qa::QaApplicabilityAssessmentV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    crate::runtime_qa::QaApplicabilityAssessmentV1 {
        schema_version: 1,
        task_uid: uid(),
        target_id: "api-service".into(),
        app_surface: crate::project_runtime::AppSurface::HeadlessService,
        code_sha: "a".repeat(40),
        approved_spec_hash: "b".repeat(64),
        applicability: crate::runtime_qa::QaApplicability::NotApplicable,
        reason: "The selected target is headless; this does not certify service tests.".into(),
        evidence_refs: vec!["target:api-service".into()],
        reviewer_execution_id: uid(),
        reviewer_session_id: uid(),
        approved_by: uid(),
        manager_session_id: uid(),
        recorded_at_unix_ms: 1,
    }
}

/// Production boundary: independent RuntimeQaReviewer role → validated verdict → durable local proposal.
/// Unit validation alone cannot prove the review process is isolated, carries the typed role manifest, and
/// preserves reviewer/Manager identity bindings in its receipt. This fixture does not start a provider,
/// Docker image, or project process; the real harness is selected by the application. External dependencies:
/// none; fake harness plus temporary storage, low setup cost.
#[test]
fn independent_headless_applicability_review_creates_a_durable_proposal_only() {
    let assessment = assessment();
    let assessment_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&assessment).unwrap())
    );
    let identity = ReviewExecutionIdentity {
        execution_id: assessment.reviewer_execution_id.clone(),
        session_id: assessment.reviewer_session_id.clone(),
    };
    let response = serde_json::json!({
        "subject_hash": assessment_hash,
        "pass": true,
        "coverage": [{
            "requirement": "VISUAL_QA_APPLICABILITY",
            "status": "pass",
            "evidence": ["headless-target:api-service"]
        }],
        "findings": [],
        "untested": []
    })
    .to_string();
    let handle = ApplicabilityReviewHandle::start(
        assessment,
        4,
        "workspace-fixture".into(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
        "Persisted acceptance criteria: service behavior is verified separately.".into(),
        std::env::temp_dir(),
        Box::new(FakeReviewer { response, identity }),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let proposal = loop {
        if let Some(result) = handle.take_result() {
            break result.unwrap();
        }
        assert!(Instant::now() < deadline, "reviewer did not finish");
        std::thread::sleep(Duration::from_millis(5));
    };
    proposal.validate().unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = crate::runtime_qa::RuntimeQaStore::new(root.path());
    let hash = store.save_applicability_proposal(&proposal).unwrap();
    assert_eq!(
        store
            .load_applicability_proposal(&proposal.assessment.task_uid, &hash)
            .unwrap(),
        proposal
    );
    assert!(
        store
            .latest_applicability_proposal(
                &proposal.assessment.task_uid,
                &"c".repeat(40),
                &proposal.assessment.approved_spec_hash,
            )
            .unwrap()
            .is_none()
    );
}
