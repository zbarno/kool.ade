use crate::{
    project_runtime::{
        DiscoveryContext, ExecutionRecipeV1, NetworkRequirement, RecipeSource, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1,
    },
    runtime_qa::*,
};
use sha2::Digest;
use std::io::Cursor;

pub(super) struct ObservedWebDriver;

impl QaScenarioDriver for ObservedWebDriver {
    fn platform(&self) -> QaPlatform {
        QaPlatform::Web
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        let QaTargetAuthorization::Web(lease) = authorization else {
            return Err("web driver received a non-web authorization".into());
        };
        if lease.task_build_sha != request.build_artifact_sha256
            || lease.startup_recipe_ref != request.startup_recipe_ref
            || lease.startup_recipe_sha256 != request.startup_recipe_sha256
        {
            return Err("QA authorization is not bound to the approved runtime target".into());
        }

        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(8, 8)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        let bytes = png.into_inner();
        let screenshot_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let hash = format!("{:x}", sha2::Sha256::digest(&bytes));
        Ok(QaDriverOutputV1 {
            driver_version: request.driver_profile_version.clone(),
            completed: true,
            failure: None,
            display: QaDisplayV1 {
                width: Some(1024),
                height: Some(768),
                dpi: Some(96),
                theme: "light".into(),
                locale: "en-US".into(),
            },
            actions: vec![QaActionEvidenceV1 {
                index: 0,
                scenario_id: scenario.id.clone(),
                command: "screenshot".into(),
                args_redacted: Default::default(),
                pre_state: Some("python web target ready".into()),
                post_state: Some("approved page rendered".into()),
                evidence_ids: vec![screenshot_id.clone()],
                occurred_at_unix_ms: 1,
            }],
            screenshots: vec![QaDriverScreenshotV1 {
                id: screenshot_id.clone(),
                scenario_id: scenario.id.clone(),
                sha256: hash,
                width: 8,
                height: 8,
                source_action: 0,
                redaction_masks: vec![],
                bytes,
            }],
            assertions: vec![QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: scenario.requirement_ids.clone(),
                expected: "approved page rendered".into(),
                observed: "approved page rendered".into(),
                status: QaAssertionStatus::Passed,
                evidence_refs: vec![screenshot_id],
            }],
            logs: vec![],
            crashes: vec![],
            browser_console: vec![],
            failed_requests: vec![],
        })
    }
}

pub(super) fn approved_recipe(
    id: &str,
    tool: &str,
    approval: &str,
    outputs: &[&str],
) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: tool.into(),
        argv: vec!["server.py".into()],
        allowed_env_keys: vec![],
        allowed_output_paths: outputs.iter().map(|path| (*path).into()).collect(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: outputs.iter().map(|path| (*path).into()).collect(),
        cache_scope: None,
        prerequisite_tool_ids: vec![tool.into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![approval.into(), "pyproject.toml".into()],
    }
}

pub(super) fn receipt(
    context: &DiscoveryContext,
    target_id: &str,
    operation: RuntimeOperation,
    recipe_id: &str,
    artifact_path: &str,
    artifact_hash: &str,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        operation,
        task_id: context.owning_task_id.clone(),
        target_id: target_id.into(),
        source_sha: context.source_sha.clone(),
        recipe_id: Some(recipe_id.into()),
        state: RuntimeReceiptState::Passed,
        exit_code: Some(0),
        artifact_hashes: [(artifact_path.into(), artifact_hash.into())]
            .into_iter()
            .collect(),
        prepared_dependency_ids: Vec::new(),
        log_reference: Some(format!("synthetic-{recipe_id}")),
        observed_at_unix_ms: 1,
    }
}

pub(super) fn qa_request(
    context: &DiscoveryContext,
    build_hash: &str,
    recipe_ref: &str,
    recipe_hash: &str,
) -> QaRunRequestV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: context.owning_task_id.clone(),
        feature_uid: uid(),
        code_head_sha: context.source_sha.clone(),
        build_artifact_sha256: build_hash.into(),
        complete_diff_hash: "d".repeat(64),
        review_subject_sha256: "e".repeat(64),
        approved_spec_hash: "f".repeat(64),
        requirement_ids: vec!["REQ-PY-WEB".into()],
        acceptance_criteria: vec!["Python web target is visible in browser QA".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: recipe_ref.into(),
        startup_recipe_sha256: recipe_hash.into(),
        execution_session_id: uid(),
        driver_profile_version: "web-driver/v1".into(),
        test_data_fixture_ref: Some("synthetic:python-web-fixture".into()),
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 8,
            max_actions_per_scenario: 16,
            max_total_actions: 64,
            timeout_seconds: 120,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 8,
        },
        author_run_id: uid(),
        author_session_id: uid(),
    }
}

pub(super) fn reviewer_prompt(
    request: &QaRunRequestV1,
    run: &str,
    session: &str,
) -> crate::harness::prompt_catalog::PromptManifest {
    use crate::harness::prompt_catalog::{EvidenceItem, RoleInputV1, RolePayloadV1, render};
    render(&RoleInputV1 {
        role: crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2,
        workspace_uid: request.feature_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: "Review synthetic Python web UI behavior".into(),
        approved_spec_ref: Some("synthetic-spec".into()),
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: vec![],
        evidence_manifest: Vec::<EvidenceItem>::new(),
        execution_policy_ref: "read-only isolated reviewer".into(),
        output_schema_version: "ReviewVerdictV1".into(),
        payload: RolePayloadV1::Review {
            subject_revision: "synthetic-build".into(),
            subject_hash: request.code_head_sha.clone(),
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: run.into(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(session.into()),
            requirements_matrix: request.acceptance_criteria.clone(),
            explicit_exclusions: vec![],
        },
    })
    .unwrap()
    .manifest
}

pub(super) fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
