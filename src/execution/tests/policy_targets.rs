use super::super::{contract::ExecutionModeV1, policy::ExecutionPolicyV1};
use super::{budget, request};
use std::collections::BTreeSet;

#[test]
fn request_working_directory_cannot_escape_the_task_workspace() {
    let mut request = request();
    request.working_directory_relative = "packages/web".into();
    request.validate_at(1).unwrap();
    request.working_directory_relative = "packages/../../outside".into();
    assert!(request.validate_at(1).is_err());
    request.working_directory_relative = "/etc".into();
    assert!(request.validate_at(1).is_err());
}

#[test]
fn scoped_write_grant_is_typed_relative_and_default_denied() {
    let root_scoped = ExecutionPolicyV1::for_scoped_workspace_write(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        ".".into(),
    )
    .unwrap();
    assert!(root_scoped.validate().is_ok());
    assert_eq!(
        root_scoped.workspace_write_scope().unwrap().root_relative,
        "."
    );
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
    for invalid in ["./", "", "../outside", "apps/../outside", "/tmp/outside"] {
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
fn repository_root_scope_requires_an_exact_user_approval_reference() {
    let root_scoped = ExecutionPolicyV1::for_approved_recipe(
        budget(1024 * 1024 * 1024, 2000),
        ".".into(),
        "user-approval:root-target".into(),
        "node".into(),
        vec!["build.js".into()],
        ".".into(),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(root_scoped.validate().is_ok());
    for (root, approval) in [
        ("./", "user-approval:root-target"),
        ("../outside", "user-approval:root-target"),
        ("/workspace", "user-approval:root-target"),
        (".", "project-config:untrusted"),
    ] {
        assert!(
            ExecutionPolicyV1::for_approved_recipe(
                budget(1024 * 1024 * 1024, 2000),
                root.into(),
                approval.into(),
                "node".into(),
                vec!["build.js".into()],
                ".".into(),
                BTreeSet::new(),
            )
            .is_err(),
            "unexpectedly accepted root={root:?} approval={approval:?}"
        );
    }
}
