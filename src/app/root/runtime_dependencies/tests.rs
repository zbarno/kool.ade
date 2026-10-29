use super::*;

#[test]
fn prepared_dependency_label_removes_controls_from_package_and_version() {
    let request = DependencyRequest {
        id: "55555555-5555-4555-8555-555555555555".into(),
        task_id: "22222222-2222-4222-8222-222222222222".into(),
        need: crate::harness::DependencyNeed {
            ecosystem: crate::harness::PackageEcosystem::Npm,
            package: Some("safe\npackage".into()),
            version: Some("1.0\u{7f}0".into()),
            source: None,
            command: String::new(),
            reason: String::new(),
            kind: crate::harness::DependencyKind::NewProjectDependency,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        },
        category: crate::harness::DependencyFailureCategory::Unknown,
        decision: crate::harness::DependencyDecision::AuthorizeForTask,
        rationale: String::new(),
        risk: String::new(),
        status: DependencyRequestStatus::Prepared,
        preparation: None,
    };

    let label = dependency_label(&request);

    assert_eq!(label, "npm: safepackage@1.00");
    assert!(!label.chars().any(char::is_control));
}

#[test]
fn selected_runtime_dependencies_must_be_unique_prepared_request_ids() {
    let first = "11111111-1111-4111-8111-111111111111".to_owned();
    let second = "22222222-2222-4222-8222-222222222222".to_owned();
    let available = vec![first.clone(), second.clone()];

    assert_eq!(
        validate_selected_dependency_ids(&[second.clone(), first.clone()], &available).unwrap(),
        [first.clone(), second.clone()]
    );
    assert!(
        validate_selected_dependency_ids(
            &["33333333-3333-4333-8333-333333333333".into()],
            &available
        )
        .is_err()
    );
    assert!(validate_selected_dependency_ids(&[first.clone(), first], &available).is_err());
    assert!(
        validate_selected_dependency_ids(&["npm:typescript".into()], &["npm:typescript".into()])
            .is_err()
    );
}

#[test]
fn cache_inventory_keeps_the_newest_snapshot_for_each_daemon_architecture() {
    let cache_ref =
        |target_arch: &str, lock_hash: &str| crate::execution::contract::DependencyCacheRefV1 {
            ecosystem: crate::execution::contract::DependencyCacheEcosystemV1::Npm,
            task_id: "11111111-1111-4111-8111-111111111111".into(),
            project_id: "22222222-2222-4222-8222-222222222222".into(),
            resolved_integrity: "sha256:lock".into(),
            target_arch: target_arch.into(),
            target_id: "local".into(),
            manifest_sha256: lock_hash.repeat(64),
            size_bytes: 1,
        };
    let older_amd64 = cache_ref("amd64", "a");
    let arm64 = cache_ref("arm64", "b");
    let newest_amd64 = cache_ref("x86_64", "c");

    let selected = distinct_arch_cache_refs([&newest_amd64, &arm64, &older_amd64].into_iter());

    assert_eq!(selected, [newest_amd64, arm64]);
}

#[test]
fn prepared_cache_must_match_the_runtime_planning_item_identity() {
    let task_uid = "11111111-1111-4111-8111-111111111111";
    let planning_uid = "22222222-2222-4222-8222-222222222222";
    let project_uid = "33333333-3333-4333-8333-333333333333";
    let task = crate::core::lifecycle::TaskRecord {
        schema_version: 1,
        task_uid: task_uid.into(),
        feature_uid: "44444444-4444-4444-8444-444444444444".into(),
        planning_item_uid: Some(planning_uid.into()),
        task_spec_sha256: None,
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: crate::core::lifecycle::TaskPhase::Ready,
        revision: 0,
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
    let cache_ref =
        |request_task_id: &str, ecosystem| crate::execution::contract::DependencyCacheRefV1 {
            ecosystem,
            task_id: request_task_id.into(),
            project_id: project_uid.into(),
            resolved_integrity: "sha256:lock".into(),
            target_arch: "amd64".into(),
            target_id: "local".into(),
            manifest_sha256: "a".repeat(64),
            size_bytes: 1,
        };
    let request = |request_task_id: &str, ecosystem| DependencyRequest {
        id: "55555555-5555-4555-8555-555555555555".into(),
        task_id: request_task_id.into(),
        need: crate::harness::DependencyNeed {
            ecosystem,
            package: Some("synthetic-package".into()),
            version: None,
            source: None,
            command: String::new(),
            reason: String::new(),
            kind: crate::harness::DependencyKind::ExistingRestore,
            lockfile_identity: Some("sha256:lock".into()),
            introduced_packages: Vec::new(),
        },
        category: crate::harness::DependencyFailureCategory::Unknown,
        decision: crate::harness::DependencyDecision::AuthorizeForTask,
        rationale: String::new(),
        risk: String::new(),
        status: DependencyRequestStatus::Prepared,
        preparation: None,
    };

    assert!(!valid_cache_ref(
        &request(task_uid, crate::harness::PackageEcosystem::Npm),
        &task,
        project_uid,
        "sha256:lock",
        &cache_ref(
            task_uid,
            crate::execution::contract::DependencyCacheEcosystemV1::Npm
        )
    ));
    assert!(valid_cache_ref(
        &request(planning_uid, crate::harness::PackageEcosystem::Npm),
        &task,
        project_uid,
        "sha256:lock",
        &cache_ref(
            planning_uid,
            crate::execution::contract::DependencyCacheEcosystemV1::Npm
        )
    ));
    assert!(!valid_cache_ref(
        &request(
            "66666666-6666-4666-8666-666666666666",
            crate::harness::PackageEcosystem::Npm
        ),
        &task,
        project_uid,
        "sha256:lock",
        &cache_ref(
            "66666666-6666-4666-8666-666666666666",
            crate::execution::contract::DependencyCacheEcosystemV1::Npm
        )
    ));

    assert!(valid_cache_ref(
        &request(planning_uid, crate::harness::PackageEcosystem::Pip),
        &task,
        project_uid,
        "sha256:lock",
        &cache_ref(
            planning_uid,
            crate::execution::contract::DependencyCacheEcosystemV1::Pip
        )
    ));
    assert!(!valid_cache_ref(
        &request(planning_uid, crate::harness::PackageEcosystem::Pip),
        &task,
        project_uid,
        "sha256:lock",
        &cache_ref(
            planning_uid,
            crate::execution::contract::DependencyCacheEcosystemV1::Cargo
        )
    ));
}
