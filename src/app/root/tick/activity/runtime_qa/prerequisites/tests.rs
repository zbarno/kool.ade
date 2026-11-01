use super::*;

#[test]
fn headless_service_uses_its_surface_and_the_browser_qa_profile() {
    assert_eq!(
        super::requested_surface(&[crate::runtime_qa::QaPlatform::HeadlessService]),
        Some(crate::project_runtime::AppSurface::HeadlessService)
    );
    assert_eq!(
        super::profile_for_platforms(&[crate::runtime_qa::QaPlatform::HeadlessService]),
        Some(RuntimeProfile::BrowserQa)
    );
}

#[test]
fn prerequisite_assessment_rejects_mixed_or_unsupported_platform_sets() {
    assert_eq!(
        super::requested_surface(&[
            crate::runtime_qa::QaPlatform::Web,
            crate::runtime_qa::QaPlatform::HeadlessService,
        ]),
        None
    );
    assert_eq!(
        super::profile_for_platforms(&[
            crate::runtime_qa::QaPlatform::HeadlessService,
            crate::runtime_qa::QaPlatform::LinuxDesktop
        ]),
        None
    );
}

fn dependency_request(
    id: &str,
    task_id: &str,
    status: crate::harness::DependencyRequestStatus,
    preparation_status: Option<crate::harness::DependencyPreparationStatus>,
) -> crate::harness::DependencyRequest {
    crate::harness::DependencyRequest {
        id: id.into(),
        task_id: task_id.into(),
        need: crate::harness::DependencyNeed {
            ecosystem: crate::harness::PackageEcosystem::Npm,
            package: Some("example".into()),
            version: Some("1.0.0".into()),
            source: Some("https://registry.npmjs.org".into()),
            command: "npm install example".into(),
            reason: "Needed by the approved application target".into(),
            kind: crate::harness::DependencyKind::NewProjectDependency,
            lockfile_identity: Some("sha256:lockfile".into()),
            introduced_packages: Vec::new(),
        },
        category: crate::harness::DependencyFailureCategory::Unknown,
        decision: crate::harness::DependencyDecision::AuthorizeForTask,
        rationale: "Approved for this task".into(),
        risk: "Bounded to this task".into(),
        status,
        preparation: preparation_status.map(|status| {
            crate::harness::DependencyPreparationTelemetry {
                status: Some(status),
                ..Default::default()
            }
        }),
    }
}

#[test]
fn dependency_inventory_uses_activity_keyed_by_the_task_document_path() {
    let mut identity = crate::domain::ArtifactIdentity::new("TASK-1", "Synthetic target");
    identity.parent_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "repo", Vec::new()).unwrap();
    let target = crate::project_runtime::AppTargetV1 {
        target_id: "target".into(),
        owning_task_id: identity.uid,
        repository_id: "repo".into(),
        app_surface: crate::project_runtime::AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: None,
        test_recipe_refs: Vec::new(),
        start_recipe_ref: None,
        readiness_probe_ref: None,
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: crate::project_runtime::TargetCapabilityState::ReadyButUntested,
    };
    let document = crate::artifacts::task_docs::TaskDocument {
        path: "planning/tasks/TASK-1.md".into(),
        title: "Synthetic target".into(),
        text: String::new(),
        identity: None,
        metadata: Some(metadata),
        task_state: None,
        metadata_error: None,
    };
    let activity = crate::harness::LiveProgress::default();
    let activities = [(document.path.clone(), activity)].into_iter().collect();

    assert!(task_progress_for_target(&[document], &activities, &target).is_some());
}

#[test]
fn only_declared_successfully_prepared_task_dependencies_are_available() {
    let prepared_id = "55555555-5555-4555-8555-555555555555";
    let task = TaskRecord {
        schema_version: 1,
        task_uid: "11111111-1111-4111-8111-111111111111".into(),
        feature_uid: "feature".into(),
        planning_item_uid: Some("22222222-2222-4222-8222-222222222222".into()),
        task_spec_sha256: None,
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa,
        revision: 1,
        approved_plan_sha256: "a".repeat(64),
        implementation_sha256: None,
        reviewed_code_head_sha: None,
        reviewed_code_subject_sha256: None,
        code_review: None,
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: None,
        qa_execution: None,
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: None,
    };
    let target = crate::project_runtime::AppTargetV1 {
        target_id: "target".into(),
        owning_task_id: "22222222-2222-4222-8222-222222222222".into(),
        repository_id: "repo".into(),
        app_surface: crate::project_runtime::AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: Some("build".into()),
        test_recipe_refs: vec!["test".into()],
        start_recipe_ref: Some("start".into()),
        readiness_probe_ref: Some("ready".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: vec![prepared_id.into(), "authorized".into()],
        supported_driver_ids: vec![],
        port_needs: vec![],
        environment_policy_ref: "default-deny".into(),
        capability_state: crate::project_runtime::TargetCapabilityState::Ready,
    };
    let progress = crate::harness::LiveProgress {
        dependency_requests: vec![
            dependency_request(
                prepared_id,
                "22222222-2222-4222-8222-222222222222",
                crate::harness::DependencyRequestStatus::Prepared,
                Some(crate::harness::DependencyPreparationStatus::Prepared),
            ),
            dependency_request(
                "authorized",
                "22222222-2222-4222-8222-222222222222",
                crate::harness::DependencyRequestStatus::Authorized,
                Some(crate::harness::DependencyPreparationStatus::AuthorizationRequired),
            ),
            dependency_request(
                "foreign",
                "33333333-3333-4333-8333-333333333333",
                crate::harness::DependencyRequestStatus::Prepared,
                Some(crate::harness::DependencyPreparationStatus::Prepared),
            ),
            dependency_request(
                "missing-telemetry",
                "22222222-2222-4222-8222-222222222222",
                crate::harness::DependencyRequestStatus::Prepared,
                None,
            ),
        ],
        ..Default::default()
    };
    let cache_root = tempfile::tempdir().unwrap();
    let cache_source = tempfile::tempdir().unwrap();
    std::fs::write(cache_source.path().join("index.cache"), b"prepared cache").unwrap();
    let snapshot = crate::harness::resource_bridge::CacheSnapshot::capture(
        cache_source.path(),
        "22222222-2222-4222-8222-222222222222",
        crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        "66666666-6666-4666-8666-666666666666",
        "sha256:lockfile",
        "amd64",
        "local",
    )
    .unwrap();
    let mut progress = progress;
    progress.dependency_requests[0]
        .preparation
        .as_mut()
        .unwrap()
        .cache_refs
        .push(snapshot.cache_ref.clone());
    let cache_store =
        crate::execution::DependencyCacheSnapshotStore::new(cache_root.path().join("snapshots"));
    cache_store.put(prepared_id, &snapshot).unwrap();

    assert_eq!(
        available_broker_dependencies(
            &target,
            &task,
            "66666666-6666-4666-8666-666666666666",
            Some(&progress),
            &cache_store
        ),
        BTreeSet::from([prepared_id.into()])
    );
    assert!(
        available_broker_dependencies(
            &target,
            &task,
            "66666666-6666-4666-8666-666666666666",
            None,
            &cache_store
        )
        .is_empty()
    );
}

#[test]
fn capability_inventory_uses_only_locked_tools_and_enabled_qa_drivers() {
    let lock = ImageLock::bundled().unwrap();
    let (tools, drivers) = locked_capabilities(&lock);
    let mut expected_tools = BTreeSet::new();
    for profile in [
        RuntimeProfile::General,
        RuntimeProfile::Rust,
        RuntimeProfile::Node,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
    ] {
        for architecture in ["amd64", "arm64"] {
            if let Ok(image) = lock.select(profile, architecture) {
                expected_tools.extend(
                    image
                        .tool_versions
                        .into_keys()
                        .filter(|tool| !matches!(tool.as_str(), "base" | "shell")),
                );
            }
        }
    }
    assert_eq!(tools, expected_tools);
    assert!(!tools.contains("base"));
    assert!(!tools.contains("shell"));
    for (driver, profile) in [
        ("web-driver", RuntimeProfile::BrowserQa),
        ("linux-desktop-driver", RuntimeProfile::DesktopQa),
    ] {
        let enabled = ["amd64", "arm64"]
            .into_iter()
            .any(|architecture| lock.select(profile, architecture).is_ok());
        assert_eq!(drivers.contains(driver), enabled);
    }
}
