use crate::runtime_qa::*;

fn save_infrastructure_report(store: &RuntimeQaStore, task_uid: &str, request: &QaRunRequestV1) {
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: Vec::new(),
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::NavigateAllowedUrl {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("the view is available".into()),
            screenshot_checkpoint: false,
        }],
        expected_results: vec!["The view is available".into()],
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
        actions: Vec::new(),
        screenshots: Vec::new(),
        assertions: vec![QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario.id.clone()],
            requirement_ids: request.requirement_ids.clone(),
            expected: "The view is available".into(),
            observed: "The driver could not start".into(),
            status: QaAssertionStatus::Blocked,
            evidence_refs: Vec::new(),
        }],
        logs: Vec::new(),
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
        coverage: Vec::new(),
        retention_policy: "user-local/30-days".into(),
    };
    let result = QaResultV1 {
        schema_version: 1,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        outcome: QaOutcome::InfrastructureBlocked,
        evidence_manifest_sha256: evidence.sha256().unwrap(),
        cases: vec![QaCaseResultV1 {
            scenario_id: scenario.id.clone(),
            status: QaCaseStatus::Blocked,
            evidence_refs: Vec::new(),
            observations: vec!["The driver could not start".into()],
        }],
        coverage: QaCoverageV1 {
            requirements: request
                .requirement_ids
                .iter()
                .map(|requirement_id| QaRequirementCoverageV1 {
                    requirement_id: requirement_id.clone(),
                    status: QaCoverageStatus::Blocked,
                    evidence_refs: Vec::new(),
                })
                .collect(),
            scenario_ids: vec![scenario.id.clone()],
            states: Vec::new(),
        },
        findings: Vec::new(),
        tool_versions: [("driver".into(), request.driver_profile_version.clone())].into(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 2,
    };
    store
        .save_run(request, &[scenario], &evidence, &result)
        .unwrap();
}

fn save_failed_report_without_receipt(
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
            observed: "The target did not launch, so the requirement was not observed".into(),
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
        findings: Vec::new(),
        tool_versions: [("driver".into(), request.driver_profile_version.clone())].into(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 2,
    };
    store
        .save_run(request, &[scenario], &evidence, &result)
        .unwrap();
}

#[path = "completed_report/cases.rs"]
mod cases;
