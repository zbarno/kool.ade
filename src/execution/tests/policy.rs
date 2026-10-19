use super::*;

#[test]
fn verification_policy_allows_the_python_and_go_stack_recipes() {
    let policy = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(policy.command_capabilities().contains("python"));
    assert!(policy.command_capabilities().contains("go"));
}

#[test]
fn scoped_write_grant_is_typed_relative_and_default_denied() {
    let valid = ExecutionPolicyV1::for_scoped_workspace_write(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        "apps/web".into(),
    )
    .unwrap();
    assert_eq!(
        valid.workspace_write_scope().unwrap().root_relative,
        "apps/web"
    );
    assert!(valid.validate().is_ok());
    let project_root = ExecutionPolicyV1::for_scoped_workspace_write(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        ".".into(),
    )
    .unwrap();
    assert_eq!(
        project_root.workspace_write_scope().unwrap().root_relative,
        "."
    );
    assert!(project_root.validate().is_ok());
    for invalid in ["", "../outside", "apps/../outside", "/tmp/outside"] {
        assert!(
            ExecutionPolicyV1::for_scoped_workspace_write(
                ExecutionModeV1::Verification,
                budget(1024 * 1024 * 1024, 2000),
                invalid.into(),
            )
            .is_err(),
            "unexpectedly accepted {invalid:?}"
        );
    }
    assert!(
        ExecutionPolicyV1::for_mode(
            ExecutionModeV1::Verification,
            budget(1024 * 1024 * 1024, 2000),
            BTreeSet::new(),
        )
        .unwrap()
        .workspace_write_scope()
        .is_none()
    );
}

#[test]
fn approved_recipe_request_binds_exact_command_arguments_and_target_scope() {
    let mut request = request();
    request.mode = ExecutionModeV1::Verification;
    request.working_directory_relative = ".".into();
    request.argv = vec!["odin".into(), "build".into(), "src".into()];
    request.environment_allowlist.clear();
    request.policy = ExecutionPolicyV1::for_approved_recipe(
        budget(1024 * 1024 * 1024, 2000),
        "apps/odin".into(),
        "user-approval:decision-1".into(),
        "odin".into(),
        vec!["build".into(), "src".into()],
        ".".into(),
        BTreeSet::new(),
    )
    .unwrap()
    .with_artifact_paths(vec!["dist/index.html".into()])
    .unwrap();
    assert_eq!(request.policy.artifact_paths(), &["dist/index.html"]);
    request.validate_at(1).unwrap();

    let ceiling = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Verification,
        budget(2 * 1024 * 1024 * 1024, 4000),
        BTreeSet::new(),
    )
    .unwrap();
    request.validate_under_policy(1, &ceiling).unwrap();

    request.argv[2] = "../sibling".into();
    assert!(request.validate_at(1).is_err());
}

#[test]
fn artifact_collection_policy_rejects_unbounded_or_unsafe_paths() {
    let base = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(
        base.clone()
            .with_artifact_paths(vec!["dist/../secrets".into()])
            .is_err()
    );
    assert!(
        base.clone()
            .with_artifact_paths(vec!["dist/index.html".into(), "dist/index.html".into()])
            .is_err()
    );
    assert!(
        base.with_artifact_paths((0..129).map(|index| format!("dist/{index}.bin")).collect())
            .is_err()
    );
    assert!(
        ExecutionPolicyV1::for_mode(
            ExecutionModeV1::RuntimeQa,
            budget(1024 * 1024 * 1024, 2000),
            BTreeSet::new(),
        )
        .unwrap()
        .with_artifact_paths(vec!["dist/index.html".into()])
        .is_ok()
    );
    assert!(
        ExecutionPolicyV1::for_mode(
            ExecutionModeV1::PlanningReadOnly,
            budget(1024 * 1024 * 1024, 2000),
            BTreeSet::new(),
        )
        .unwrap()
        .with_artifact_paths(vec!["dist/index.html".into()])
        .is_err()
    );
}

#[test]
fn role_selection_maps_to_security_modes_without_provider_authority() {
    assert_eq!(
        HarnessRole::Implementer.execution_mode(),
        ExecutionModeV1::Implementation
    );
    assert_eq!(
        HarnessRole::RuntimeQaReviewer.execution_mode(),
        ExecutionModeV1::RuntimeQa
    );
    assert_eq!(
        HarnessRole::RuntimeQaReviewer.harness_mode(),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
    for role in [
        HarnessRole::Manager,
        HarnessRole::PlanningAnalyst,
        HarnessRole::PlanReviewer,
        HarnessRole::TaskDecomposer,
        HarnessRole::CodeReviewer,
        HarnessRole::SpecReconciler,
    ] {
        assert_eq!(role.execution_mode(), ExecutionModeV1::PlanningReadOnly);
    }
    assert_eq!(
        HarnessRole::PRPreparer.execution_mode(),
        ExecutionModeV1::PlanningReadOnly
    );
    assert_eq!(
        HarnessRole::PRPreparer.harness_mode(),
        crate::harness::ExecutionMode::ReadOnlyAnalysis
    );
    assert!(!HarnessRole::PRPreparer.requires_execution_target());
    assert_eq!(
        HarnessRole::RepairWorker.execution_mode(),
        ExecutionModeV1::Implementation
    );
}

#[test]
fn policy_intersection_never_widens_capabilities_or_resource_limits() {
    let broad = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Implementation,
        budget(4 * 1024 * 1024 * 1024, 4000),
        BTreeSet::new(),
    )
    .unwrap();
    let mut narrow_json = serde_json::to_value(&broad).unwrap();
    narrow_json["command_capabilities"] = serde_json::json!(["git"]);
    narrow_json["tool_capabilities"] = serde_json::json!(["read_repository"]);
    narrow_json["filesystem"] = serde_json::json!(["workspace_read"]);
    let narrow: ExecutionPolicyV1 = serde_json::from_value(narrow_json).unwrap();
    let effective = broad.restrict_to(&narrow).unwrap();
    assert_eq!(
        effective.command_capabilities(),
        &BTreeSet::from(["git".into()])
    );
    assert_eq!(
        effective.tool_capabilities(),
        &BTreeSet::from(["read_repository".into()])
    );
    assert!(
        effective
            .filesystem_grants()
            .is_subset(broad.filesystem_grants())
    );
    assert!(
        effective
            .filesystem_grants()
            .is_subset(narrow.filesystem_grants())
    );
    assert!(effective.resource_limits().memory_bytes <= broad.resource_limits().memory_bytes);
    assert!(effective.resource_limits().memory_bytes <= narrow.resource_limits().memory_bytes);

    let mut widened_json = serde_json::to_value(&narrow).unwrap();
    widened_json["command_capabilities"] = serde_json::json!(["docker"]);
    let widened: ExecutionPolicyV1 = serde_json::from_value(widened_json).unwrap();
    assert!(widened.validate().is_err());

    let request = request();
    assert!(request.validate_under_policy(1, &broad).is_ok());
    let low_ceiling = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Implementation,
        budget(512 * 1024 * 1024, 1000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(request.validate_under_policy(1, &low_ceiling).is_err());
}
