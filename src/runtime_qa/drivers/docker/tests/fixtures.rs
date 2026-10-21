use crate::{
    execution::errors::{ExecutionError, ExecutionErrorKind},
    runtime_qa::{
        QaActionEvidenceV1, QaAssertionV1, QaDisplayV1, QaDriverOutputV1, QaDriverScreenshotV1,
        QaPlatform, QaRunRequestV1, QaScenarioV1, QaValidatedTargetV1,
    },
};

pub(super) fn driver_output() -> QaDriverOutputV1 {
    QaDriverOutputV1 {
        driver_version: "linux-x11-driver/v1".into(),
        completed: true,
        failure: None,
        display: QaDisplayV1 {
            width: 1440,
            height: 900,
            dpi: 96,
            theme: "light".into(),
            locale: "en-US".into(),
        },
        actions: Vec::<QaActionEvidenceV1>::new(),
        screenshots: Vec::<QaDriverScreenshotV1>::new(),
        assertions: Vec::<QaAssertionV1>::new(),
        logs: Vec::new(),
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
    }
}

pub(super) fn request() -> QaRunRequestV1 {
    QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uuid(),
        parent_task_uid: uuid(),
        planning_item_uid: None,
        feature_uid: uuid(),
        code_head_sha: "b".repeat(40),
        build_artifact_sha256: "c".repeat(64),
        complete_diff_hash: "d".repeat(64),
        review_subject_sha256: "e".repeat(64),
        approved_spec_hash: "f".repeat(64),
        requirement_ids: vec!["REQ-1".into()],
        acceptance_criteria: vec!["fixture opens".into()],
        relevant_platforms: vec![QaPlatform::LinuxDesktop],
        startup_recipe_ref: "desktop-start".into(),
        startup_recipe_sha256: "1".repeat(64),
        execution_session_id: uuid(),
        driver_profile_version: "linux-x11-driver/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 4,
            max_actions_per_scenario: 8,
            max_total_actions: 16,
            timeout_seconds: 60,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 4,
        },
        author_run_id: uuid(),
        author_session_id: uuid(),
    }
}

pub(super) fn target(request: &QaRunRequestV1) -> QaValidatedTargetV1 {
    QaValidatedTargetV1 {
        task_uid: request.parent_task_uid.clone(),
        build_artifact_sha256: request.build_artifact_sha256.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        executable: "/workspace/app".into(),
        working_directory: "/workspace".into(),
        arguments: Vec::new(),
    }
}

pub(super) fn scenario() -> QaScenarioV1 {
    QaScenarioV1 {
        schema_version: 1,
        id: uuid(),
        requirement_ids: vec!["REQ-1".into()],
        preconditions: Vec::new(),
        steps: Vec::new(),
        expected_results: vec!["fixture opens".into()],
        negative_paths: Vec::new(),
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    }
}

pub(super) fn uuid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

pub(super) fn failure(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::WorkerIncompatible, message, false)
}
