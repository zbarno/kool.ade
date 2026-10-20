use super::*;

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
