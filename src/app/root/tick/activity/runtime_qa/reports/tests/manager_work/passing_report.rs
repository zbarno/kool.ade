use crate::runtime_qa::*;

/// Persist a deterministic, independently reviewed headless pass bound to the supplied request.
pub(super) fn save(store: &RuntimeQaStore, request: &QaRunRequestV1) -> QaResultV1 {
    let scenario_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let evidence_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: scenario_id.clone(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["Approved service build is ready".into()],
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::ApiRequest {
                method: QaHttpMethodV1::Get,
                path: "/health".into(),
                json_body: None,
                expected_status: 200,
                expected_body_contains: None,
            },
            expected_state: Some("health request returns HTTP 200".into()),
            screenshot_checkpoint: false,
        }],
        expected_results: vec!["Health request returns HTTP 200".into()],
        negative_paths: Vec::new(),
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    };
    let evidence = QaEvidenceManifestV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        target_id: request.target_id.clone(),
        code_sha: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        build_hash: request.build_artifact_sha256.clone(),
        platform: QaPlatform::HeadlessService,
        driver_version: request.driver_profile_version.clone(),
        display: QaDisplayV1 {
            width: None,
            height: None,
            dpi: None,
            theme: "headless-service".into(),
            locale: "en-US".into(),
        },
        scenario_ids: vec![scenario_id.clone()],
        actions: vec![QaActionEvidenceV1 {
            index: 0,
            scenario_id: scenario_id.clone(),
            command: "api_request".into(),
            args_redacted: Default::default(),
            pre_state: Some("service ready".into()),
            post_state: Some("HTTP 200".into()),
            evidence_ids: vec![evidence_id.clone()],
            occurred_at_unix_ms: 1,
        }],
        screenshots: Vec::new(),
        assertions: vec![QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario_id.clone()],
            requirement_ids: request.requirement_ids.clone(),
            expected: "Health request returns HTTP 200".into(),
            observed: "Received HTTP 200".into(),
            status: QaAssertionStatus::Passed,
            evidence_refs: vec![evidence_id.clone()],
        }],
        logs: Vec::new(),
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
        coverage: request.requirement_ids.clone(),
        retention_policy: "user-local/30-days".into(),
    };
    let result = QaResultV1 {
        schema_version: CURRENT_QA_RESULT_SCHEMA_VERSION,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        outcome: QaOutcome::Pass,
        evidence_manifest_sha256: evidence.sha256().unwrap(),
        scenario_set_sha256: scenario_set_sha256(std::slice::from_ref(&scenario)).unwrap(),
        reviewed_screenshot_ids: Vec::new(),
        review_coverage: request
            .requirement_ids
            .iter()
            .map(|requirement_id| QaReviewRequirementCoverageV1 {
                requirement_id: requirement_id.clone(),
                status: QaReviewCoverageStatusV1::Pass,
                evidence_refs: vec!["0".into(), evidence_id.clone()],
            })
            .collect(),
        cases: vec![QaCaseResultV1 {
            scenario_id: scenario_id.clone(),
            status: QaCaseStatus::Passed,
            evidence_refs: vec![evidence_id.clone()],
            observations: vec!["HTTP 200".into()],
        }],
        coverage: QaCoverageV1 {
            requirements: request
                .requirement_ids
                .iter()
                .map(|requirement_id| QaRequirementCoverageV1 {
                    requirement_id: requirement_id.clone(),
                    status: QaCoverageStatus::Observed,
                    evidence_refs: vec![evidence_id.clone()],
                })
                .collect(),
            scenario_ids: vec![scenario_id],
            states: vec!["service ready".into(), "HTTP 200".into()],
        },
        findings: Vec::new(),
        tool_versions: [("driver".into(), request.driver_profile_version.clone())].into(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 2,
    };
    store
        .save_run(request, &[scenario], &evidence, &result)
        .unwrap();
    result
}

pub(super) fn fabricated_pass(request: &QaRunRequestV1) -> QaResultV1 {
    QaResultV1 {
        schema_version: CURRENT_QA_RESULT_SCHEMA_VERSION,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        outcome: QaOutcome::Pass,
        evidence_manifest_sha256: "a".repeat(64),
        scenario_set_sha256: "b".repeat(64),
        reviewed_screenshot_ids: Vec::new(),
        review_coverage: Vec::new(),
        cases: Vec::new(),
        coverage: QaCoverageV1 {
            requirements: Vec::new(),
            scenario_ids: Vec::new(),
            states: Vec::new(),
        },
        findings: Vec::new(),
        tool_versions: Default::default(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 2,
    }
}

pub(super) fn record_fresh_pass(
    project: &mut crate::app::session::Project,
    task_uid: &str,
    original_task: &crate::core::lifecycle::TaskRecord,
    original_request: &QaRunRequestV1,
    store: &RuntimeQaStore,
) -> QaResultV1 {
    use crate::core::lifecycle::{LifecycleStore, TaskEvent};

    let planning = project.state.planning_store.clone();
    let lifecycle = LifecycleStore::new(&planning);
    let mut task = lifecycle
        .apply_task_event(
            task_uid,
            lifecycle.load_task(task_uid).unwrap().revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            TaskEvent::BeginRepair,
        )
        .unwrap();
    let repaired_code_hash = "f".repeat(64);
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            TaskEvent::ImplementationFinished {
                code_sha256: repaired_code_hash.clone(),
            },
        )
        .unwrap();
    let reviewed_head = "9".repeat(40);
    let reviewed_subject = "8".repeat(64);
    let review = fresh_code_review_receipt(
        task_uid,
        &original_task.feature_uid,
        &original_task.approved_plan_sha256,
        &repaired_code_hash,
        &reviewed_head,
        &reviewed_subject,
    );
    let author_run_id = review.author_run_id.clone().unwrap();
    let author_session_id = review.author_session_id.clone().unwrap();
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            TaskEvent::CodeReview {
                receipt: review,
                reviewed_head_sha: reviewed_head.clone(),
                review_subject_sha256: reviewed_subject.clone(),
            },
        )
        .unwrap();

    let mut request = original_request.clone();
    request.qa_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    request.code_head_sha = reviewed_head;
    request.complete_diff_hash = repaired_code_hash;
    request.review_subject_sha256 = reviewed_subject;
    request.relevant_platforms = vec![QaPlatform::HeadlessService];
    request.driver_profile_version = "headless-service/v1".into();
    request.startup_recipe_ref = "recipe:headless-service/v1".into();
    request.acceptance_criteria = vec!["Health request returns HTTP 200".into()];
    request.author_run_id = author_run_id;
    request.author_session_id = author_session_id;
    task = lifecycle
        .queue_runtime_qa(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            request.clone(),
        )
        .unwrap();
    let result = save(store, &request);
    super::super::receipt_order::save_valid_gate_receipt(store, &request);
    super::super::super::record_persisted_report(
        &planning,
        &mut project.planning_work,
        &mut project.state.baseline_planning_revision,
        &lifecycle,
        store,
        &task,
    )
    .unwrap()
    .expect("the accepted pass report is recorded");
    crate::app::manager::finish_qa_finding_repair(project, task_uid, &request, &result);
    result
}

fn fresh_code_review_receipt(
    task_uid: &str,
    feature_uid: &str,
    approved_plan_sha: &str,
    implementation_sha: &str,
    reviewed_head: &str,
    reviewed_subject: &str,
) -> crate::core::lifecycle::GateReceipt {
    use crate::{
        core::lifecycle::{GateActor, GateDecision, GateReceipt},
        harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render},
    };

    let role = PromptRole::CodeReviewer;
    let author_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let author_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = render(&RoleInputV1 {
        role,
        workspace_uid: feature_uid.into(),
        feature_uid: feature_uid.into(),
        task_uid: Some(task_uid.into()),
        original_goal: "Repair the blocking Runtime QA finding".into(),
        approved_spec_ref: Some("approved-plan".into()),
        approved_spec_hash: Some(approved_plan_sha.into()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "review/fresh-runtime-qa-repair/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: reviewed_head.into(),
            subject_hash: reviewed_subject.into(),
            author_run_id: author_run_id.clone(),
            reviewer_run_id: reviewer_run_id.clone(),
            author_session_id: Some(author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.clone()),
            requirements_matrix: vec![format!("Repaired implementation {implementation_sha}")],
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.into(),
        subject_sha256: implementation_sha.into(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 3,
        evidence_refs: vec!["fixture:fresh-code-review".into()],
        prompt_sha256: Some(prompt.identity_sha256()),
        prompt_manifest: Some(prompt),
        reviewer_run_id: Some(reviewer_run_id),
        author_run_id: Some(author_run_id),
        reviewer_session_id: Some(reviewer_session_id),
        author_session_id: Some(author_session_id),
    }
}
