use super::*;
use sha2::Digest as _;

#[test]
fn immutable_approved_run_persists_report_and_returns_gate_receipt() {
    let (request, mut scenarios, lease) = fixture();
    scenarios[0].redaction_regions = vec![crate::runtime_qa::QaRedactionRegionV1 {
        x: 2,
        y: 3,
        width: 4,
        height: 2,
    }];
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Passed,
        failure_before_actions: false,
        failure_on_call: None,
    };
    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute(QaExecutionContext {
            request: &request,
            scenarios: &scenarios,
            authorization: QaTargetAuthorization::Web(&lease),
            driver: &mut driver,
            reviewer_execution_id: &reviewer_run,
            reviewer_session_id: &reviewer_session,
            prompt,
        })
        .unwrap();
    assert_eq!(result.outcome, QaOutcome::Pass);
    assert_eq!(
        receipt.as_ref().unwrap().subject_sha256,
        request.code_head_sha
    );
    assert_eq!(driver.calls, 1);
    assert_eq!(store.load_run(&request.qa_run_id).unwrap().3, result);
    let persisted = store.load_run_with_screenshots(&request.qa_run_id).unwrap();
    assert_eq!(
        persisted.evidence.screenshots[0].redaction_masks,
        ["rect:2,3,4,2"]
    );
    let image = image::load_from_memory(&persisted.screenshots[0].bytes)
        .unwrap()
        .to_rgba8();
    assert_eq!(image.get_pixel(2, 3), &image::Rgba([0, 0, 0, 255]));
    assert_eq!(image.get_pixel(5, 4), &image::Rgba([0, 0, 0, 255]));
    assert_eq!(image.get_pixel(0, 0), &image::Rgba([240, 120, 60, 255]));
    assert_eq!(
        persisted.evidence.screenshots[0].sha256,
        format!(
            "{:x}",
            sha2::Sha256::digest(&persisted.screenshots[0].bytes)
        )
    );
    assert_eq!(
        Some(store.load_gate_receipt(&request.qa_run_id).unwrap()),
        receipt
    );
}

#[test]
// Production boundary: a desktop/browser driver that fails before its first
// action still produces a durable, blocked QA report with sanitized diagnostics.
// A validator unit test cannot prove orchestration persists the failure with
// exact task/spec bindings. Uses in-process adapters and a temporary directory;
// no Docker, SSH, network, or credentials, with low setup cost.
fn driver_start_failure_before_first_action_is_persisted_as_infrastructure_blocked() {
    let (request, scenarios, lease) = fixture();
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Untested,
        failure_before_actions: false,
        failure_on_call: Some(1),
    };
    let mut reviewer = |_evidence: &QaEvidenceManifestV1, _candidate: &QaResultV1| {
        Err("no runtime observations are available to review".into())
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

    assert_eq!(driver.calls, 1);
    assert_eq!(result.outcome, QaOutcome::InfrastructureBlocked);
    assert!(receipt.is_none());
    assert!(
        result
            .cases
            .iter()
            .all(|case| case.status == QaCaseStatus::Blocked)
    );
    assert!(result.cases[0].observations.iter().any(|item| {
        item.contains("display did not start")
            && !item.contains("/private/worker")
            && !item.contains("token=secret")
            && !item.contains("abc123")
    }));
    assert!(evidence_has_blocked_assertion(&store, &request.qa_run_id));
    let (stored_request, _, evidence, stored_result) = store.load_run(&request.qa_run_id).unwrap();
    assert_eq!(stored_request, request);
    assert!(evidence.actions.is_empty());
    assert!(evidence.logs.iter().any(|line| line.contains("[redacted]")));
    assert!(evidence.logs.iter().all(|line| !line.contains("abc123")));
    assert_eq!(stored_result, result);
    let mut forged_pass = stored_result;
    forged_pass.outcome = QaOutcome::Pass;
    assert!(
        crate::runtime_qa::validate_report(&request, &scenarios, &evidence, &forged_pass).is_err()
    );
}

fn evidence_has_blocked_assertion(store: &RuntimeQaStore, run_id: &str) -> bool {
    store.load_run(run_id).is_ok_and(|(_, _, evidence, _)| {
        evidence
            .assertions
            .iter()
            .any(|assertion| assertion.status == QaAssertionStatus::Blocked)
    })
}

#[test]
fn observed_failed_assertion_is_a_blocking_manager_finding() {
    let (request, scenarios, lease) = fixture();
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

    let (result, _) = RuntimeQaOrchestrator::new(&store)
        .execute(QaExecutionContext {
            request: &request,
            scenarios: &scenarios,
            authorization: QaTargetAuthorization::Web(&lease),
            driver: &mut driver,
            reviewer_execution_id: &reviewer_run,
            reviewer_session_id: &reviewer_session,
            prompt,
        })
        .unwrap();

    assert_eq!(result.outcome, QaOutcome::Fail);
    assert_eq!(result.findings.len(), 1);
    let finding = &result.findings[0];
    assert!(finding.blocking);
    assert_eq!(finding.requirement_uids, request.requirement_ids);
    assert_eq!(finding.screenshot_ids.len(), 1);
    assert!(!finding.trace_ids.is_empty());
    let stored = store.load_run(&request.qa_run_id).unwrap().3;
    assert_eq!(stored.findings, result.findings);
}
