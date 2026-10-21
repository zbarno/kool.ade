use super::*;

fn run_passing_reviewer(
    assertion_status: QaAssertionStatus,
) -> (
    QaResultV1,
    Option<crate::core::lifecycle::GateReceipt>,
    RuntimeQaStore,
    tempfile::TempDir,
    String,
) {
    let (request, scenarios, lease) = fixture();
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status,
    };
    let mut reviewer = |evidence: &QaEvidenceManifestV1, _candidate: &QaResultV1| {
        Ok(ReviewVerdictV1 {
            subject_hash: evidence.sha256()?,
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
            coverage: vec![CoverageV1 {
                requirement: request.requirement_ids[0].clone(),
                status: CoverageStatus::Pass,
                evidence: vec![evidence.actions[0].index.to_string()],
            }],
            findings: vec![],
            untested: vec![],
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

#[test]
fn independent_reviewer_rejection_prevents_driver_candidate_pass() {
    let (request, scenarios, lease) = fixture();
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Passed,
    };
    let mut reviewer = |evidence: &QaEvidenceManifestV1, _candidate: &QaResultV1| {
        Ok(ReviewVerdictV1 {
            subject_hash: evidence.sha256()?,
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: false,
            outcome: None,
            coverage: vec![CoverageV1 {
                requirement: request.requirement_ids[0].clone(),
                status: CoverageStatus::Fail,
                evidence: vec![evidence.actions[0].index.to_string()],
            }],
            findings: vec![],
            untested: vec![],
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

    assert_eq!(result.outcome, QaOutcome::Fail);
    assert_eq!(result.findings.len(), 1);
    assert!(result.findings[0].blocking);
    assert_eq!(
        receipt.unwrap().decision,
        crate::core::lifecycle::GateDecision::ChangesRequested
    );
    assert_eq!(
        store.load_run(&request.qa_run_id).unwrap().3.outcome,
        QaOutcome::Fail
    );
}

#[test]
fn author_session_cannot_execute_qa_or_invoke_the_driver() {
    let (request, scenarios, lease) = fixture();
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(
        &request,
        &reviewer_run,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Passed,
    };
    assert!(
        RuntimeQaOrchestrator::new(&store)
            .execute(QaExecutionContext {
                request: &request,
                scenarios: &scenarios,
                authorization: QaTargetAuthorization::Web(&lease),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_run,
                reviewer_session_id: &request.author_session_id,
                prompt,
            },)
            .is_err()
    );
    assert_eq!(driver.calls, 0);
}

#[test]
fn passing_reviewer_cannot_upgrade_incomplete_or_failed_driver_evidence() {
    for (driver_status, expected_outcome, expects_gate) in [
        (QaAssertionStatus::Untested, QaOutcome::Incomplete, false),
        (QaAssertionStatus::Failed, QaOutcome::Fail, true),
    ] {
        let (result, receipt, store, _temp, run_id) = run_passing_reviewer(driver_status);
        assert_eq!(result.outcome, expected_outcome);
        assert_eq!(receipt.is_some(), expects_gate);
        assert_eq!(store.load_run(&run_id).unwrap().3, result);
        if let Some(receipt) = receipt {
            assert_eq!(
                receipt.decision,
                crate::core::lifecycle::GateDecision::ChangesRequested
            );
        }
    }
}

#[test]
fn failed_reviewer_persists_observed_driver_evidence_as_blocked() {
    let (request, scenarios, lease) = fixture();
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Passed,
    };
    let mut reviewer = |_evidence: &QaEvidenceManifestV1, _candidate: &QaResultV1| {
        Err("synthetic reviewer transport failure".into())
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
    let (saved_request, saved_scenarios, evidence, saved_result) =
        store.load_run(&request.qa_run_id).unwrap();
    assert_eq!(saved_result, result);
    assert_eq!(evidence.actions.len(), 2);
    assert_eq!(evidence.screenshots.len(), 1);
    assert_eq!(
        evidence.coverage,
        vec![format!("{}:Blocked", request.requirement_ids[0])]
    );
    assert_eq!(
        evidence.logs.last().map(String::as_str),
        Some(
            "independent runtime QA review could not be completed; result is infrastructure blocked"
        )
    );
    result
        .validate(&saved_request, &saved_scenarios, &evidence)
        .unwrap();
}
