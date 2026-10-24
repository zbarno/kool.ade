use super::*;
use crate::harness::prompt_catalog::{
    CoverageStatus, CoverageV1, ReviewFindingV1, ReviewVerdictV1,
};

/// Production boundary: combine failed driver assertions with the independent
/// review before persisting one report. A converter-only unit test cannot prove
/// the orchestration avoids duplicate findings. This uses the in-memory QA
/// store and synthetic driver/reviewer, without Docker, network, or API keys.
#[test]
fn reviewer_correctness_finding_enriches_matching_failed_assertion_once() {
    let (request, mut scenarios, lease) = fixture();
    let second_scenario_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut second_scenario = scenarios[0].clone();
    second_scenario.id = second_scenario_id.clone();
    scenarios.push(second_scenario);
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Failed,
        failure_before_actions: false,
        failure_on_call: None,
    };
    let mut reviewer =
        |evidence: &QaEvidenceManifestV1,
         candidate: &QaResultV1,
         _screenshots: &[crate::runtime_qa::QaScreenshotArtifactV1]| {
            let screenshot = evidence
                .screenshots
                .iter()
                .find(|shot| shot.scenario_id == second_scenario_id)
                .unwrap();
            let action = evidence
                .actions
                .iter()
                .find(|action| action.scenario_id == second_scenario_id)
                .unwrap();
            Ok(ReviewVerdictV1 {
                subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                    &request, candidate, evidence,
                )?,
                examined_files: Vec::new(),
                tested_evidence_ids: vec![screenshot.id.clone()],
                unexamined_areas: Vec::new(),
                pass: false,
                outcome: None,
                coverage: vec![CoverageV1 {
                    requirement: request.requirement_ids[0].clone(),
                    status: CoverageStatus::Fail,
                    evidence: vec![action.index.to_string(), screenshot.id.clone()],
                }],
                findings: vec![ReviewFindingV1 {
                    id: "review-finding-1".into(),
                    requirement: request.requirement_ids[0].clone(),
                    severity: crate::harness::prompt_catalog::Severity::High,
                    kind: crate::harness::prompt_catalog::FindingKind::Correctness,
                    scope: Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
                    location: "observed acceptance result".into(),
                    location_detail: None,
                    evidence: vec![action.index.to_string()],
                    repro: Some("The acceptance criterion does not hold.".into()),
                    proposed_correction: "Correct the observed keyboard dismissal behavior.".into(),
                    rationale: "The observed result contradicts the requirement.".into(),
                    affected_requirement_ids: vec![request.requirement_ids[0].clone()],
                    suggested_action: Some(
                        crate::harness::prompt_catalog::ReviewFindingActionV1::Repair,
                    ),
                }],
                untested: Vec::new(),
            })
        };
    let (result, _) = RuntimeQaOrchestrator::new(&store)
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

    assert_eq!(result.outcome, QaOutcome::Fail);
    assert_eq!(result.findings.len(), 2);
    let matching = result
        .findings
        .iter()
        .find(|finding| finding.scenario_uid == second_scenario_id)
        .unwrap();
    assert_eq!(
        matching.smallest_fix,
        "Correct the observed keyboard dismissal behavior."
    );
    assert!(
        matching
            .reproducible_steps
            .iter()
            .any(|step| step == "The acceptance criterion does not hold.")
    );
    assert!(
        result
            .findings
            .iter()
            .any(|finding| finding.scenario_uid == scenarios[0].id)
    );
    assert_eq!(store.load_run(&request.qa_run_id).unwrap().3, result);
}
