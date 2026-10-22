use super::*;
use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::lifecycle::{QaExecutionStateV1, QaExecutionStatusV1},
    core::planning::QuestionStore,
    runtime_qa::*,
};

#[test]
// Production boundary: a failed report without its required reviewer receipt
// must not create Manager decisions before rejection. Uses file-backed QA and
// planning stores with synthetic evidence; no Docker or network setup.
fn missing_gate_receipt_prevents_durable_finding_decisions() {
    let root = tempfile::tempdir().unwrap();
    let planning = PlanningStore::new(uuid::Uuid::new_v4(), root.path(), StoreMode::ManagedLocal);
    let mut task = super::task();
    let request = super::request(&task);
    task.qa_execution = Some(QaExecutionStateV1 {
        schema_version: 1,
        status: QaExecutionStatusV1::Running,
        request: Some(request.clone()),
        request_task_revision: None,
        dispatch_task_revision: None,
        blocked_reason: None,
        result_sha256: None,
        evidence_manifest_sha256: None,
        reviewer_execution_id: None,
        reviewer_session_id: None,
        applicability: None,
    });
    let qa_root = root.path().join("qa");
    let qa_store = RuntimeQaStore::new(&qa_root);
    save_failed_report_with_nonblocking_finding(&qa_store, &task.task_uid, &request);

    let result = record_persisted_report(
        &planning,
        &crate::core::lifecycle::LifecycleStore::new(&planning),
        &qa_store,
        &task,
    );

    assert!(result.is_err());
    assert!(QuestionStore::new(&planning).list().unwrap().is_empty());
    assert!(
        !qa_root
            .join(&request.qa_run_id)
            .join("dispositions")
            .exists()
    );
}

fn save_failed_report_with_nonblocking_finding(
    store: &RuntimeQaStore,
    task_uid: &str,
    request: &QaRunRequestV1,
) {
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: Vec::new(),
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/".into(),
            },
            expected_state: None,
            screenshot_checkpoint: false,
        }],
        expected_results: vec!["The target launches".into()],
        negative_paths: Vec::new(),
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    };
    let evidence = QaEvidenceManifestV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: task_uid.into(),
        code_sha: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        build_hash: request.build_artifact_sha256.clone(),
        platform: QaPlatform::Web,
        driver_version: request.driver_profile_version.clone(),
        display: QaDisplayV1 {
            width: 1024,
            height: 768,
            dpi: 96,
            theme: "light".into(),
            locale: "en-US".into(),
        },
        scenario_ids: vec![scenario.id.clone()],
        actions: vec![QaActionEvidenceV1 {
            index: 0,
            scenario_id: scenario.id.clone(),
            command: "open".into(),
            args_redacted: Default::default(),
            pre_state: None,
            post_state: None,
            evidence_ids: Vec::new(),
            occurred_at_unix_ms: 1,
        }],
        screenshots: Vec::new(),
        assertions: vec![QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario.id.clone()],
            requirement_ids: request.requirement_ids.clone(),
            expected: "The target launches".into(),
            observed: "The driver did not produce a complete observation".into(),
            status: QaAssertionStatus::Blocked,
            evidence_refs: Vec::new(),
        }],
        logs: Vec::new(),
        crashes: vec!["synthetic driver crash".into()],
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
        coverage: Vec::new(),
        retention_policy: "user-local/30-days".into(),
    };
    let mut finding = super::finding(false);
    finding.scenario_uid = scenario.id.clone();
    let result = QaResultV1 {
        schema_version: 1,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        outcome: QaOutcome::Fail,
        evidence_manifest_sha256: evidence.sha256().unwrap(),
        cases: vec![QaCaseResultV1 {
            scenario_id: scenario.id.clone(),
            status: QaCaseStatus::Skipped,
            evidence_refs: Vec::new(),
            observations: vec!["The target did not launch".into()],
        }],
        coverage: QaCoverageV1 {
            requirements: request
                .requirement_ids
                .iter()
                .map(|requirement_id| QaRequirementCoverageV1 {
                    requirement_id: requirement_id.clone(),
                    status: QaCoverageStatus::Untested,
                    evidence_refs: Vec::new(),
                })
                .collect(),
            scenario_ids: vec![scenario.id.clone()],
            states: Vec::new(),
        },
        findings: vec![finding],
        tool_versions: [("driver".into(), request.driver_profile_version.clone())].into(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 2,
    };
    store
        .save_run(request, &[scenario], &evidence, &result)
        .unwrap();
}
