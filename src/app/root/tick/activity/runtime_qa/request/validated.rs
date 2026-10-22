use crate::{
    core::lifecycle::{GateDecision, TaskPhase},
    project_runtime::{AppSurface, RuntimeOperation, RuntimeReceiptState},
    runtime_qa::{QaBoundedLimitsV1, QaDependencyCacheRefV1, QaPlatform, QaRunRequestV1},
};
use sha2::Digest;

use super::ReviewedTaskInputs;
use super::artifact::artifact_bundle_sha256;
use super::task_spec::{acceptance_criteria, hash_task_spec};

pub(super) fn from_reviewed_task(inputs: ReviewedTaskInputs<'_>) -> Result<QaRunRequestV1, String> {
    let ReviewedTaskInputs {
        task,
        request_task_revision,
        task_text,
        implementation,
        target,
        startup_recipe_sha256,
        build,
        artifact_paths,
        driver_profile_version,
        dependency_cache_refs,
    } = inputs;
    if task.phase != TaskPhase::AwaitingRuntimeQa
        || task.planning_item_uid.as_deref() != implementation.task_uid.as_deref()
        || implementation.verified_head.as_deref() != Some(build.source_sha.as_str())
        || task.reviewed_code_head_sha.as_deref() != Some(build.source_sha.as_str())
        || task.implementation_sha256.as_deref()
            != task
                .code_review
                .as_ref()
                .map(|review| review.subject_sha256.as_str())
        || task.code_review.as_ref().is_none_or(|review| {
            review.decision != GateDecision::Approved
                || review.subject_uid != task.task_uid
                || review.author_run_id.is_none()
                || review.author_session_id.is_none()
                || review.reviewer_run_id.is_none()
                || review.reviewer_session_id.is_none()
        })
    {
        return Err("runtime QA requires the exact approved lifecycle review and verified implementation head".into());
    }
    let spec_sha = task
        .task_spec_sha256
        .as_deref()
        .ok_or_else(|| "runtime QA task has no immutable task-specification hash".to_owned())?;
    if hash_task_spec(&task.approved_plan_sha256, task_text)? != spec_sha {
        return Err(
            "runtime QA task text differs from its immutable reviewed specification".into(),
        );
    }
    let criteria = acceptance_criteria(task_text)?;
    if target.owning_task_id != task.planning_item_uid.as_deref().unwrap_or_default()
        || target.start_recipe_ref.is_none()
        || !matches!(
            target.capability_state,
            crate::project_runtime::TargetCapabilityState::Ready
                | crate::project_runtime::TargetCapabilityState::ReadyButUntested
        )
        || build.task_id != target.owning_task_id
        || build.target_id != target.target_id
        || build.operation != RuntimeOperation::Build
        || build.state != RuntimeReceiptState::Passed
        || build.exit_code != Some(0)
        || build.source_sha != implementation.verified_head.as_deref().unwrap_or_default()
        || build.recipe_id != target.build_recipe_ref
        || artifact_paths.is_empty()
        || build.artifact_hashes.len() != artifact_paths.len()
        || artifact_paths
            .iter()
            .any(|path| !build.artifact_hashes.contains_key(path))
    {
        return Err("runtime QA has no successful build receipt for this reviewed task, head, and approved target".into());
    }
    if driver_profile_version.trim().is_empty()
        || startup_recipe_sha256.len() != 64
        || !startup_recipe_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("trusted runtime QA driver profile identity is invalid".into());
    }
    let artifact_sha256 = artifact_bundle_sha256(artifact_paths, &build.artifact_hashes)?;
    let platform = match target.app_surface {
        AppSurface::Web => QaPlatform::Web,
        AppSurface::LinuxDesktop => QaPlatform::LinuxDesktop,
        _ => {
            return Err(
                "the approved application target has no supported runtime QA driver".into(),
            );
        }
    };
    let review = task.code_review.as_ref().expect("validated above");
    let cache_refs = if platform == QaPlatform::Web {
        dependency_cache_refs
            .iter()
            .map(
                |(dependency_request_id, cache_ref)| QaDependencyCacheRefV1 {
                    dependency_request_id: dependency_request_id.clone(),
                    cache_ref: cache_ref.clone(),
                },
            )
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let cache_identity = format!(
        "{:x}",
        sha2::Sha256::digest(
            serde_json::to_vec(&cache_refs)
                .map_err(|_| { "runtime QA dependency cache identity could not be encoded" })?
        )
    );
    let execution_key = format!(
        "runtime-qa-v1:{}:{}:{}:{}",
        request_task_revision,
        build.source_sha,
        task.implementation_sha256.as_deref().unwrap_or_default(),
        cache_identity,
    );
    let namespace =
        uuid::Uuid::parse_str(&task.task_uid).map_err(|_| "runtime QA task identity is invalid")?;
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uuid::Uuid::new_v5(&namespace, execution_key.as_bytes())
            .hyphenated()
            .to_string(),
        parent_task_uid: task.task_uid.clone(),
        planning_item_uid: task.planning_item_uid.clone(),
        feature_uid: task.feature_uid.clone(),
        code_head_sha: task
            .reviewed_code_head_sha
            .clone()
            .expect("reviewed head validated above"),
        build_artifact_sha256: artifact_sha256,
        complete_diff_hash: task.implementation_sha256.clone().expect("reviewed hash"),
        review_subject_sha256: task
            .reviewed_code_subject_sha256
            .clone()
            .ok_or("reviewed code subject hash is missing from lifecycle state")?,
        approved_spec_hash: task.approved_plan_sha256.clone(),
        requirement_ids: criteria
            .iter()
            .enumerate()
            .map(|(i, _)| format!("criterion-{}", i + 1))
            .collect(),
        acceptance_criteria: criteria,
        relevant_platforms: vec![platform],
        startup_recipe_ref: target.start_recipe_ref.clone().expect("validated above"),
        startup_recipe_sha256: startup_recipe_sha256.to_owned(),
        execution_session_id: uuid::Uuid::new_v5(
            &namespace,
            format!("{execution_key}:session").as_bytes(),
        )
        .hyphenated()
        .to_string(),
        driver_profile_version: driver_profile_version.to_owned(),
        test_data_fixture_ref: target.fixture_ref.clone(),
        dependency_cache_refs: cache_refs,
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 32,
            max_actions_per_scenario: 64,
            max_total_actions: 512,
            timeout_seconds: 900,
            max_evidence_bytes: 64 * 1024 * 1024,
            max_screenshots: 128,
        },
        author_run_id: review.author_run_id.clone().expect("validated above"),
        author_session_id: review.author_session_id.clone().expect("validated above"),
    };
    request.validate()?;
    Ok(request)
}
