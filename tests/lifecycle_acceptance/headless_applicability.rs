use super::{plan, support::*};
use crate::task_review;
use koolade::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleActor, LifecycleEventKind, LifecycleStore,
        QaExecutionStatusV1, TaskEvent, TaskPhase,
    },
    project_runtime::{
        AppSurface, AppTargetV1, ProjectCapabilityManifestV1, ProjectRuntimeStore,
        TargetCapabilityState,
    },
    runtime_qa::{QaApplicability, QaApplicabilityAssessmentV1},
};
use sha2::Digest as _;

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
        build_recipe_ref: None,
        test_recipe_refs: Vec::new(),
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
    ProjectRuntimeStore::new(&store)
        .save_manifest(
            &ProjectCapabilityManifestV1 {
                schema_version: 1,
                workspace_id: id(),
                repository_id: "repo-main".into(),
                source_sha: "a".repeat(40),
                detected_languages: Vec::new(),
                frameworks: Vec::new(),
                candidate_targets: vec![target],
                selected_target_id: Some("api-service".into()),
                target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
                recipes: Vec::new(),
                readiness_probes: Default::default(),
                artifact_specs: Default::default(),
                confidence: 50,
                evidence_refs: Vec::new(),
                user_overrides: Vec::new(),
                revision: 0,
            },
            0,
        )
        .unwrap();
    let assessment = QaApplicabilityAssessmentV1 {
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
        ],
        reviewer_execution_id: id(),
        reviewer_session_id: id(),
        approved_by: id(),
        manager_session_id: id(),
        recorded_at_unix_ms: 1,
    };
    let assessment_hash = format!(
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
    let receipt = GateReceipt {
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
