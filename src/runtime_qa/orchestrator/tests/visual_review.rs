use super::*;
use crate::harness::prompt_catalog::{CoverageStatus, CoverageV1, ReviewVerdictV1};

fn run_visual_review(
    visual_scenario_count: usize,
    cited_scenario_count: usize,
    inspected_scenario_count: usize,
    driver_assertion_status: QaAssertionStatus,
) -> (
    QaResultV1,
    Option<crate::core::lifecycle::GateReceipt>,
    RuntimeQaStore,
    tempfile::TempDir,
    String,
) {
    let (request, mut scenarios, lease) = fixture();
    scenarios[0].visual_expectations =
        vec!["Text and controls are readable without overlap".into()];
    while scenarios.len() < visual_scenario_count {
        let mut scenario = scenarios[0].clone();
        scenario.id = uuid::Uuid::new_v4().hyphenated().to_string();
        scenarios.push(scenario);
    }
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: driver_assertion_status,
        failure_before_actions: false,
        failure_on_call: None,
    };
    let mut reviewer =
        |evidence: &QaEvidenceManifestV1,
         candidate: &QaResultV1,
         screenshots: &[crate::runtime_qa::QaScreenshotArtifactV1]| {
            assert_eq!(candidate.outcome, QaOutcome::Incomplete);
            assert_eq!(
                candidate.coverage.requirements[0].status,
                crate::runtime_qa::QaCoverageStatus::Untested
            );
            let cited_screenshots = screenshots
                .iter()
                .take(cited_scenario_count)
                .collect::<Vec<_>>();
            assert!(!cited_screenshots.is_empty());
            let inspected_screenshots = screenshots
                .iter()
                .take(inspected_scenario_count)
                .collect::<Vec<_>>();
            for screenshot in &inspected_screenshots {
                assert!(image::load_from_memory(&screenshot.bytes).is_ok());
            }
            let mut evidence_refs = Vec::new();
            for screenshot in &cited_screenshots {
                let metadata = evidence
                    .screenshots
                    .iter()
                    .find(|item| item.id == screenshot.id)
                    .unwrap();
                evidence_refs.push(metadata.source_action.to_string());
                evidence_refs.push(screenshot.id.clone());
            }
            Ok(ReviewVerdictV1 {
                subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                    &request, candidate, evidence,
                )?,
                examined_files: Vec::new(),
                tested_evidence_ids: inspected_screenshots
                    .iter()
                    .map(|screenshot| screenshot.id.clone())
                    .collect(),
                unexamined_areas: Vec::new(),
                pass: true,
                outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
                coverage: vec![CoverageV1 {
                    requirement: request.requirement_ids[0].clone(),
                    status: CoverageStatus::Pass,
                    evidence: evidence_refs,
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
    (result, receipt, store, temp, request.qa_run_id)
}

// The orchestration boundary must accept a visual-only criterion only after an
// audited reviewer inspection; provider MCP tests separately verify tool access.
// This unit path uses in-memory QA storage and a fake reviewer, with no Docker or API.
#[test]
fn visual_only_coverage_requires_and_accepts_audited_screenshot_inspection() {
    let (result, receipt, store, _temp, run_id) =
        run_visual_review(1, 1, 1, QaAssertionStatus::Untested);
    assert_eq!(result.outcome, QaOutcome::Pass);
    assert_eq!(
        result.coverage.requirements[0].status,
        crate::runtime_qa::QaCoverageStatus::Observed
    );
    assert_eq!(
        result.review_coverage[0].status,
        crate::runtime_qa::QaReviewCoverageStatusV1::Pass
    );
    assert_eq!(
        receipt.unwrap().decision,
        crate::core::lifecycle::GateDecision::Approved
    );
    let (_, _, evidence, stored) = store.load_run(&run_id).unwrap();
    assert_eq!(evidence.assertions[0].status, QaAssertionStatus::Passed);
    assert!(evidence.logs.iter().any(|line| {
        line == "independent reviewer inspected screenshot pixels for every visual scenario"
    }));
    assert!(!evidence.logs.iter().any(|line| {
        line == super::super::VISUAL_REVIEW_PENDING_LOG
            || line.contains("selected reviewer cannot inspect screenshot pixels")
    }));
    assert!(
        evidence.assertions[0]
            .observed
            .contains("reviewer inspected the redacted screenshot")
    );
    assert_eq!(stored, result);
}

#[test]
fn a_visual_pass_without_screenshot_inspection_stays_incomplete() {
    let (result, receipt, store, _temp, run_id) =
        run_visual_review(1, 1, 0, QaAssertionStatus::Untested);
    assert_eq!(result.outcome, QaOutcome::Incomplete);
    assert!(receipt.is_none());
    assert_eq!(
        result.coverage.requirements[0].status,
        crate::runtime_qa::QaCoverageStatus::Untested
    );
    assert_eq!(
        result.review_coverage[0].status,
        crate::runtime_qa::QaReviewCoverageStatusV1::Untested
    );
    let (_, _, evidence, _) = store.load_run(&run_id).unwrap();
    assert!(evidence.logs.iter().any(|line| {
        line == "some visual expectations lack cited, reviewer-inspected screenshot evidence"
    }));
}

/// Production boundary: reviewer coverage must remain untested unless every
/// visual scenario's screenshot was audited before the lifecycle gate is built.
/// A pure helper test cannot prove that incomplete visual review with otherwise
/// passing driver assertions yields no gate receipt. This is an in-process
/// orchestration test with a temporary store; it needs no Docker, SSH, network,
/// provider credentials, or browser and has low setup cost.
#[test]
fn visual_requirement_pass_requires_inspecting_each_scenario_screenshot() {
    let (complete, receipt, _, _temp, _) = run_visual_review(2, 2, 2, QaAssertionStatus::Untested);
    assert_eq!(complete.outcome, QaOutcome::Pass);
    assert_eq!(
        receipt.unwrap().decision,
        crate::core::lifecycle::GateDecision::Approved
    );

    let (partial, receipt, store, _temp, run_id) =
        run_visual_review(2, 1, 1, QaAssertionStatus::Passed);
    assert_eq!(partial.outcome, QaOutcome::Incomplete);
    assert!(receipt.is_none());
    assert_eq!(
        partial.coverage.requirements[0].status,
        crate::runtime_qa::QaCoverageStatus::Untested
    );
    assert_eq!(
        partial.review_coverage[0].status,
        crate::runtime_qa::QaReviewCoverageStatusV1::Untested
    );
    assert_eq!(partial.reviewed_screenshot_ids.len(), 1);
    assert_eq!(store.load_run(&run_id).unwrap().3, partial);
}
