use super::{plan, support::*};
use crate::task_review;
use koolade::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleActor, LifecycleEventKind, LifecycleStore,
        QaExecutionStatusV1, TaskEvent, TaskPhase,
    },
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
    },
    runtime_qa::{QaApplicability, QaApplicabilityAssessmentV1},
};
use sha2::Digest as _;

fn runtime_recipe(recipe_id: &str) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: vec!["-m".into(), "compileall".into(), ".".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["__pycache__".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:synthetic-fixture".into()],
    }
}

fn runtime_receipt(
    task_id: &str,
    target_id: &str,
    source_sha: &str,
    operation: RuntimeOperation,
    recipe_id: &str,
    state: RuntimeReceiptState,
    observed_at_unix_ms: u64,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: id(),
        operation,
        task_id: task_id.into(),
        target_id: target_id.into(),
        source_sha: source_sha.into(),
        recipe_id: Some(recipe_id.into()),
        state,
        exit_code: Some(if state == RuntimeReceiptState::Passed {
            0
        } else {
            1
        }),
        artifact_hashes: Default::default(),
        log_reference: None,
        observed_at_unix_ms,
    }
}

/// Production boundary: reviewed task → lifecycle applicability event → reopened task/event log.
/// Unit tests cannot prove the Manager actor, immutable event, and task projection survive store
/// restart together. Deterministic in-process fixtures only; no Docker/network/model dependency;
/// setup cost is the standard lifecycle acceptance fixture.
#[test]
fn headless_visual_qa_decision_is_manager_signed_and_durable() {
    let temp = tempfile::tempdir().unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), temp.path(), StoreMode::ManagedLocal);
    let lifecycle = LifecycleStore::new(&store);
    bootstrap_product(&store);
    let feature_uid = id();
    lifecycle
        .create_feature(&new_feature(feature_uid.clone()), &id())
        .unwrap();
    let feature = plan::reviewed_feature_plan(&store, &lifecycle, &feature_uid);
    let plan_hash = feature.approved_plan_sha256.unwrap();
    let (task_uid, task) =
        task_review::reviewed_task_after_repair(&store, &feature_uid, &plan_hash);

    let target = AppTargetV1 {
        target_id: "api-service".into(),
        owning_task_id: task.task_uid.clone(),
        repository_id: "repo-main".into(),
        app_surface: AppSurface::HeadlessService,
        project_relative_root: ".".into(),
        build_recipe_ref: Some("service-build".into()),
        test_recipe_refs: vec!["service-test".into()],
        start_recipe_ref: None,
        readiness_probe_ref: None,
        artifact_spec_ref: None,
        fixture_ref: Some("service-contract-v1".into()),
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let runtime_store = ProjectRuntimeStore::new(&store);
    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: id(),
        repository_id: "repo-main".into(),
        source_sha: task.reviewed_code_head_sha.clone().unwrap(),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: vec![target],
        selected_target_id: Some("api-service".into()),
        target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
        recipes: vec![
            runtime_recipe("service-build"),
            runtime_recipe("service-test"),
        ],
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 50,
        evidence_refs: Vec::new(),
        user_overrides: Vec::new(),
        revision: 0,
    };
    runtime_store.save_manifest(&manifest, 0).unwrap();
    let manifest = runtime_store.load_manifest().unwrap().unwrap();
    let mut forged_target = manifest.candidate_targets[0].clone();
    forged_target.build_recipe_ref = None;
    forged_target.test_recipe_refs.clear();
    assert!(
        runtime_store
            .headless_service_test_receipts(&task_uid, &forged_target, &manifest.source_sha,)
            .is_err()
    );
    let manifest_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
    let mut assessment = QaApplicabilityAssessmentV1 {
        schema_version: 1,
        task_uid: task_uid.clone(),
        target_id: "api-service".into(),
        app_surface: AppSurface::HeadlessService,
        code_sha: task.reviewed_code_head_sha.clone().unwrap(),
        approved_spec_hash: task.approved_plan_sha256.clone(),
        applicability: QaApplicability::NotApplicable,
        reason: "Visual QA does not apply to a headless API target".into(),
        evidence_refs: vec![
            "target:api-service".into(),
            "fixture:service-contract/v1".into(),
            format!("runtime-manifest:{manifest_hash}"),
        ],
        reviewer_execution_id: id(),
        reviewer_session_id: id(),
        approved_by: id(),
        manager_session_id: id(),
        recorded_at_unix_ms: 1,
    };
    let mut assessment_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&assessment).unwrap())
    );
    let mut reviewer_receipt = role_receipt(
        koolade::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2,
        GateDecision::Approved,
        &task_uid,
        &assessment_hash,
    );
    reviewer_receipt.reviewer_run_id = Some(assessment.reviewer_execution_id.clone());
    reviewer_receipt.reviewer_session_id = Some(assessment.reviewer_session_id.clone());
    reviewer_receipt.author_run_id = Some(assessment.approved_by.clone());
    reviewer_receipt.author_session_id = Some(assessment.manager_session_id.clone());
    reviewer_receipt.evidence_refs = vec![format!(
        "runtime-qa-applicability-assessment:{assessment_hash}"
    )];
    let mut receipt = GateReceipt {
        receipt_uid: id(),
        subject_uid: task_uid.clone(),
        subject_sha256: task.implementation_sha256.clone().unwrap(),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::NotApplicable,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("runtime-qa-applicability:{assessment_hash}"),
            format!("gate-receipt:{}", reviewer_receipt.receipt_uid),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: Some(assessment.approved_by.clone()),
        author_run_id: None,
        reviewer_session_id: Some(assessment.manager_session_id.clone()),
        author_session_id: None,
    };
    assert!(
        lifecycle
            .record_runtime_qa_applicability(
                &task_uid,
                task.revision,
                &id(),
                assessment.clone(),
                reviewer_receipt.clone(),
                receipt.clone(),
            )
            .is_err()
    );
    let build_receipt = runtime_receipt(
        &task_uid,
        "api-service",
        &assessment.code_sha,
        RuntimeOperation::Build,
        "service-build",
        RuntimeReceiptState::Passed,
        10,
    );
    let failed_test = runtime_receipt(
        &task_uid,
        "api-service",
        &assessment.code_sha,
        RuntimeOperation::Test,
        "service-test",
        RuntimeReceiptState::Failed,
        20,
    );
    runtime_store.append_receipt(&build_receipt).unwrap();
    runtime_store.append_receipt(&failed_test).unwrap();
    assert!(
        lifecycle
            .record_runtime_qa_applicability(
                &task_uid,
                task.revision,
                &id(),
                assessment.clone(),
                reviewer_receipt.clone(),
                receipt.clone(),
            )
            .is_err()
    );
    let passed_test = runtime_receipt(
        &task_uid,
        "api-service",
        &assessment.code_sha,
        RuntimeOperation::Test,
        "service-test",
        RuntimeReceiptState::Passed,
        30,
    );
    runtime_store.append_receipt(&passed_test).unwrap();
    assessment.evidence_refs.extend([
        format!("runtime-receipt:{}", build_receipt.receipt_id),
        format!("runtime-receipt:{}", passed_test.receipt_id),
    ]);
    assessment_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&assessment).unwrap())
    );
    reviewer_receipt.subject_sha256 = assessment_hash.clone();
    reviewer_receipt.evidence_refs = vec![format!(
        "runtime-qa-applicability-assessment:{assessment_hash}"
    )];
    for reference in &mut receipt.evidence_refs {
        if reference.starts_with("runtime-qa-applicability:") {
            *reference = format!("runtime-qa-applicability:{assessment_hash}");
        }
    }
    let mut forged_target = assessment.clone();
    forged_target.target_id = "api-service-forged".into();
    assert!(
        lifecycle
            .record_runtime_qa_applicability(
                &task_uid,
                task.revision,
                &id(),
                forged_target,
                reviewer_receipt.clone(),
                receipt.clone(),
            )
            .is_err()
    );
    assert_eq!(
        lifecycle.load_task(&task_uid).unwrap().revision,
        task.revision
    );

    let mut stale_manifest = manifest.clone();
    stale_manifest.source_sha = "f".repeat(40);
    runtime_store
        .save_manifest(&stale_manifest, manifest.revision)
        .unwrap();
    assert!(
        lifecycle
            .record_runtime_qa_applicability(
                &task_uid,
                task.revision,
                &id(),
                assessment.clone(),
                reviewer_receipt.clone(),
                receipt.clone(),
            )
            .is_err()
    );
    let manifest_revision = runtime_store.load_manifest().unwrap().unwrap().revision;
    runtime_store
        .save_manifest(&manifest, manifest_revision)
        .unwrap();
    let current_manifest = runtime_store.load_manifest().unwrap().unwrap();
    let current_manifest_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&current_manifest).unwrap())
    );
    assessment
        .evidence_refs
        .retain(|reference| !reference.starts_with("runtime-manifest:"));
    assessment
        .evidence_refs
        .push(format!("runtime-manifest:{current_manifest_hash}"));
    assessment_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&assessment).unwrap())
    );
    reviewer_receipt.subject_sha256 = assessment_hash.clone();
    reviewer_receipt.evidence_refs = vec![format!(
        "runtime-qa-applicability-assessment:{assessment_hash}"
    )];
    for reference in &mut receipt.evidence_refs {
        if reference.starts_with("runtime-qa-applicability:") {
            *reference = format!("runtime-qa-applicability:{assessment_hash}");
        }
    }

    let updated = lifecycle
        .record_runtime_qa_applicability(
            &task_uid,
            task.revision,
            &id(),
            assessment.clone(),
            reviewer_receipt,
            receipt,
        )
        .unwrap();
    assert_eq!(updated.phase, TaskPhase::AwaitingSpecReconciliation);
    assert_eq!(
        updated.qa_execution.as_ref().unwrap().status,
        QaExecutionStatusV1::NotApplicable
    );

    let reopened_store = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let reopened = LifecycleStore::new(&reopened_store);
    let persisted = reopened.load_task(&task_uid).unwrap();
    assert_eq!(
        persisted.qa_execution.unwrap().applicability,
        Some(assessment)
    );
    let event = reopened
        .events_for_feature(&feature_uid)
        .unwrap()
        .into_iter()
        .find(|event| {
            event.task_uid.as_deref() == Some(task_uid.as_str())
                && matches!(
                    event.event,
                    LifecycleEventKind::Task(TaskEvent::RuntimeQaNotApplicable { .. })
                )
        })
        .unwrap();
    assert_eq!(event.actor, LifecycleActor::Manager);
}
