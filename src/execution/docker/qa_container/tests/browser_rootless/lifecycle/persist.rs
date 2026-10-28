use super::{RootlessServiceOutput, reviewer_prompt};
use crate::{
    harness::prompt_catalog::{CoverageStatus, CoverageV1, ReviewOutcomeV1, ReviewVerdictV1},
    runtime_qa::{
        QaAssertionStatus, QaDriverOutputV1, QaEvidenceManifestV1, QaOutcome, QaPlatform,
        QaResultV1, QaRunRequestV1, QaScenarioV1, QaTargetAuthorization, QaTaskServiceLeaseV1,
        RuntimeQaOrchestrator, RuntimeQaStore,
    },
};

pub(crate) fn review_expected_outcome_and_persist(
    request: &QaRunRequestV1,
    scenario: QaScenarioV1,
    lease: &QaTaskServiceLeaseV1,
    output: QaDriverOutputV1,
    store_root: impl Into<std::path::PathBuf>,
    expected_outcome: QaOutcome,
) {
    assert!(matches!(
        expected_outcome,
        QaOutcome::Pass | QaOutcome::Fail
    ));
    let platform = match request.relevant_platforms.as_slice() {
        [QaPlatform::Web] => QaPlatform::Web,
        [QaPlatform::HeadlessService] => QaPlatform::HeadlessService,
        _ => panic!("rootless evidence fixture requires one supported service platform"),
    };
    let reviewer_execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(
        request,
        &scenario,
        &reviewer_execution_id,
        &reviewer_session_id,
    );

    let mut driver = RootlessServiceOutput(Some(output), platform);
    let mut reviewer =
        |evidence: &QaEvidenceManifestV1,
         candidate: &QaResultV1,
         screenshots: &[crate::runtime_qa::QaScreenshotArtifactV1]| {
            let evidence_matches_platform = match platform {
                QaPlatform::Web => !evidence.screenshots.is_empty(),
                QaPlatform::HeadlessService => {
                    evidence.screenshots.is_empty()
                        && evidence
                            .actions
                            .iter()
                            .any(|action| !action.evidence_ids.is_empty())
                }
                _ => false,
            };
            let has_expected_evidence = match expected_outcome {
                QaOutcome::Pass => candidate.outcome == QaOutcome::Pass,
                QaOutcome::Fail => {
                    candidate.outcome == QaOutcome::Fail
                        && evidence
                            .assertions
                            .iter()
                            .any(|assertion| assertion.status == QaAssertionStatus::Failed)
                }
                _ => false,
            };
            let clean_runtime = !evidence.actions.is_empty()
                && evidence_matches_platform
                && evidence.browser_console.is_empty()
                && evidence.failed_requests.is_empty();
            let observed_pass =
                expected_outcome == QaOutcome::Pass && has_expected_evidence && clean_runtime;
            assert!(
                has_expected_evidence && clean_runtime,
                "runtime QA fixture did not observe the expected platform result: expected={expected_outcome:?}, candidate={:?}, actions={}, assertions={:?}, screenshots={}, browser_console={:?}, failed_requests={:?}",
                candidate.outcome,
                evidence.actions.len(),
                evidence.assertions,
                evidence.screenshots.len(),
                evidence.browser_console,
                evidence.failed_requests
            );
            Ok(ReviewVerdictV1 {
                subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                    request, candidate, evidence,
                )?,
                examined_files: vec![],
                tested_evidence_ids: screenshots
                    .iter()
                    .map(|screenshot| screenshot.id.clone())
                    .collect(),
                unexamined_areas: vec![],
                pass: observed_pass,
                outcome: Some(if observed_pass {
                    ReviewOutcomeV1::Pass
                } else {
                    ReviewOutcomeV1::BlockingFindings
                }),
                coverage: request
                    .requirement_ids
                    .iter()
                    .map(|requirement| CoverageV1 {
                        requirement: requirement.clone(),
                        status: if observed_pass {
                            CoverageStatus::Pass
                        } else {
                            CoverageStatus::Fail
                        },
                        evidence: screenshots
                            .iter()
                            .map(|screenshot| screenshot.id.clone())
                            .chain(
                                evidence
                                    .actions
                                    .iter()
                                    .map(|action| action.index.to_string()),
                            )
                            .chain(
                                evidence
                                    .actions
                                    .iter()
                                    .flat_map(|action| action.evidence_ids.iter().cloned()),
                            )
                            .collect(),
                    })
                    .collect(),
                findings: vec![],
                untested: vec![],
            })
        };

    let store = RuntimeQaStore::new(store_root);
    let authorization = match platform {
        QaPlatform::Web => QaTargetAuthorization::Web(lease),
        QaPlatform::HeadlessService => QaTargetAuthorization::HeadlessService(lease),
        _ => unreachable!("platform was validated above"),
    };
    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            crate::runtime_qa::QaExecutionContext {
                request,
                scenarios: std::slice::from_ref(&scenario),
                authorization,
                driver: &mut driver,
                reviewer_execution_id: &reviewer_execution_id,
                reviewer_session_id: &reviewer_session_id,
                prompt,
            },
            &mut reviewer,
        )
        .expect("the live rootless service evidence is reviewable and persistable");
    let case_statuses: Vec<_> = result.cases.iter().map(|case| case.status).collect();
    let persisted = store
        .load_run_with_screenshots(&request.qa_run_id)
        .expect("the complete browser report and screenshot bytes are durable");
    assert_eq!(
        result.outcome, expected_outcome,
        "reviewed rootless QA result had unexpected case statuses: {case_statuses:?}; logs: {:?}",
        persisted.evidence.logs
    );
    assert!(
        receipt.is_some(),
        "reviewed terminal QA outcome must produce a gate receipt"
    );
    assert_eq!(persisted.request, *request);
    assert_eq!(persisted.scenarios, vec![scenario]);
    assert_eq!(persisted.result, result);
    assert_eq!(persisted.result.outcome, expected_outcome);
    match platform {
        QaPlatform::Web => assert!(!persisted.screenshots.is_empty()),
        QaPlatform::HeadlessService => assert!(persisted.screenshots.is_empty()),
        _ => unreachable!("platform was validated above"),
    }
    let receipt = receipt.expect("reviewed terminal QA result produced a gate receipt");
    assert_eq!(
        receipt.decision,
        if expected_outcome == QaOutcome::Pass {
            crate::core::lifecycle::GateDecision::Approved
        } else {
            crate::core::lifecycle::GateDecision::ChangesRequested
        }
    );
    assert_eq!(
        store.load_gate_receipt(&request.qa_run_id).unwrap(),
        receipt
    );
}
