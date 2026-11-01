use super::*;
use crate::project_runtime::{AppSurface, ReadinessProbeV1};

fn start_target(surface: AppSurface) -> (AppTargetV1, ExecutionRecipeV1) {
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "custom-start".into(),
        source: crate::project_runtime::RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: vec!["app.py".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: crate::project_runtime::NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:test".into()],
    };
    let target = AppTargetV1 {
        target_id: "custom-target".into(),
        owning_task_id: "task-custom".into(),
        repository_id: "repo-custom".into(),
        app_surface: surface,
        project_relative_root: "apps/custom".into(),
        build_recipe_ref: Some("custom-build".into()),
        test_recipe_refs: Vec::new(),
        start_recipe_ref: Some(recipe.recipe_id.clone()),
        readiness_probe_ref: None,
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: crate::project_runtime::TargetCapabilityState::ReadyButUntested,
    };
    (target, recipe)
}

#[test]
fn custom_start_readiness_must_match_surface_and_approved_executable() {
    let (web, recipe) = start_target(AppSurface::Web);
    assert!(
        validate_readiness(
            &web,
            std::slice::from_ref(&recipe),
            Some(&ReadinessProbeV1::HttpOnApprovedPort {
                port: 8080,
                path: "/health".into(),
                expected_status: 200,
            }),
        )
        .is_ok()
    );
    assert!(
        validate_readiness(
            &web,
            std::slice::from_ref(&recipe),
            Some(&ReadinessProbeV1::ProcessAlive {
                executable_id: "python".into(),
            }),
        )
        .is_err()
    );

    let (desktop, recipe) = start_target(AppSurface::LinuxDesktop);
    assert!(
        validate_readiness(
            &desktop,
            std::slice::from_ref(&recipe),
            Some(&ReadinessProbeV1::ProcessAlive {
                executable_id: "python".into(),
            }),
        )
        .is_ok()
    );
    assert!(
        validate_readiness(
            &desktop,
            &[recipe],
            Some(&ReadinessProbeV1::ProcessAlive {
                executable_id: "other".into(),
            }),
        )
        .is_err()
    );

    let (service, recipe) = start_target(AppSurface::HeadlessService);
    assert!(
        validate_readiness(
            &service,
            &[recipe],
            Some(&ReadinessProbeV1::LocalServiceHealth {
                port: 8080,
                path: "/health".into(),
            }),
        )
        .is_ok()
    );
}

#[test]
fn custom_start_requires_readiness_and_probe_requires_start() {
    let (target, recipe) = start_target(AppSurface::HeadlessService);
    assert!(validate_readiness(&target, std::slice::from_ref(&recipe), None).is_err());
    let mut target_without_build = target.clone();
    target_without_build.build_recipe_ref = None;
    assert!(
        validate_readiness(&target_without_build, std::slice::from_ref(&recipe), None)
            .unwrap_err()
            .contains("build recipe")
    );
    let mut target_without_start = target;
    target_without_start.start_recipe_ref = None;
    assert!(
        validate_readiness(
            &target_without_start,
            &[recipe],
            Some(&ReadinessProbeV1::LocalServiceHealth {
                port: 8080,
                path: "/health".into(),
            }),
        )
        .is_err()
    );

    assert!(validate_readiness(&target_without_start, &[], None).is_ok());
}

#[test]
fn repository_root_alias_passes_root_guard_before_manifest_lookup() {
    let root = std::env::temp_dir().join(format!(
        "koolade-custom-target-root-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let store = ProjectRuntimeStore::new(&planning);
    let target = AppTargetV1 {
        target_id: "user-target-root".into(),
        owning_task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        repository_id: "repo-root".into(),
        app_surface: AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: None,
        test_recipe_refs: Vec::new(),
        start_recipe_ref: None,
        readiness_probe_ref: None,
        artifact_spec_ref: Some("root-artifacts".into()),
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: crate::project_runtime::TargetCapabilityState::ReadyButUntested,
    };
    let invalid_artifact_error = store
        .add_user_target_with_readiness_and_save(
            target.clone(),
            Vec::new(),
            None,
            vec!["dist/\tindex.html".into()],
            crate::core::lifecycle::GateReceipt {
                receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                subject_uid: target.owning_task_id.clone(),
                subject_sha256: "a".repeat(64),
                actor: crate::core::lifecycle::GateActor::User,
                decision: crate::core::lifecycle::GateDecision::Approved,
                recorded_at_unix_ms: 1,
                evidence_refs: Vec::new(),
                prompt_sha256: None,
                prompt_manifest: None,
                reviewer_run_id: None,
                author_run_id: None,
                reviewer_session_id: None,
                author_session_id: None,
            },
            crate::ui::layout::RuntimeTargetApprovalProof::for_test(),
            0,
        )
        .unwrap_err()
        .to_string();
    assert!(invalid_artifact_error.contains("artifact paths"));
    let mut test_only_target = target;
    test_only_target.artifact_spec_ref = None;
    let error = store
        .add_user_target_with_readiness_and_save(
            test_only_target,
            Vec::new(),
            None,
            Vec::new(),
            crate::core::lifecycle::GateReceipt {
                receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                subject_uid: "task-root".into(),
                subject_sha256: "a".repeat(64),
                actor: crate::core::lifecycle::GateActor::User,
                decision: crate::core::lifecycle::GateDecision::Approved,
                recorded_at_unix_ms: 1,
                evidence_refs: Vec::new(),
                prompt_sha256: None,
                prompt_manifest: None,
                reviewer_run_id: None,
                author_run_id: None,
                reviewer_session_id: None,
                author_session_id: None,
            },
            crate::ui::layout::RuntimeTargetApprovalProof::for_test(),
            0,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("manifest is missing"));
    assert!(store.load_manifest().unwrap().is_none());
}
