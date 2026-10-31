use super::*;
use crate::harness::prompt_catalog::{CoverageStatus, CoverageV1, ReviewVerdictV1};

/// Production boundary: an unknown screenshot ID in an otherwise passing
/// reviewer verdict must not create a lifecycle receipt or persist inspected
/// pixel evidence. The policy runs in the orchestrator, so this in-process
/// store/driver fixture directly exercises the trust boundary without Docker,
/// SSH, network, credentials, or browser setup.
#[test]
fn forged_screenshot_id_cannot_authorize_a_visual_qa_pass() {
    let (request, mut scenarios, lease) = fixture();
    scenarios[0].visual_expectations = vec!["Controls remain readable".into()];
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Untested,
        failure_before_actions: false,
        failure_on_call: None,
    };
    let mut reviewer = |evidence: &QaEvidenceManifestV1,
                        candidate: &QaResultV1,
                        screenshots: &[QaScreenshotArtifactV1]| {
        Ok(ReviewVerdictV1 {
            subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                &request, candidate, evidence,
            )?,
            examined_files: Vec::new(),
            tested_evidence_ids: vec!["forged-screenshot-id".into()],
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
            coverage: vec![CoverageV1 {
                requirement: request.requirement_ids[0].clone(),
                status: CoverageStatus::Pass,
                evidence: vec![screenshots[0].id.clone()],
            }],
            findings: Vec::new(),
            untested: Vec::new(),
        })
    };

    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            QaExecutionContext {
                request: &request,
                scenarios: &scenarios,
                authorization: QaTargetAuthorization::Web(&lease),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_run,
                reviewer_session_id: &reviewer_session,
                prompt,
            },
            &mut reviewer,
        )
        .unwrap();

    assert_eq!(result.outcome, QaOutcome::InfrastructureBlocked);
    assert!(receipt.is_none());
    assert!(result.reviewed_screenshot_ids.is_empty());
    let (_, _, evidence, stored_result) = store.load_run(&request.qa_run_id).unwrap();
    assert_eq!(stored_result, result);
    assert_eq!(evidence.assertions[0].status, QaAssertionStatus::Untested);
    assert!(!evidence.logs.iter().any(|line| {
        line == "independent reviewer inspected screenshot pixels for every visual scenario"
    }));
    assert!(store.load_gate_receipt(&request.qa_run_id).is_err());
}
