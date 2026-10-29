use sha2::{Digest, Sha256};

use crate::project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource};

pub(super) fn approved_recipe(
    recipe_id: &str,
    executable_id: &str,
    argv: Vec<String>,
    allowed_output_paths: Vec<String>,
) -> ExecutionRecipeV1 {
    let approval_id = uuid::Uuid::new_v4().hyphenated().to_string();
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: executable_id.into(),
        argv,
        allowed_env_keys: Vec::new(),
        allowed_output_paths,
        timeout_seconds: 45,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![
            format!("user-approval:{approval_id}"),
            format!("user-recipe-approval:{approval_id}"),
        ],
    }
}

pub(super) fn source_sha() -> String {
    if let Ok(value) = std::env::var("GITHUB_SHA")
        && valid_sha(&value)
    {
        return value;
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "checked-out source revision is available"
    );
    let value = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    assert!(
        valid_sha(&value),
        "checked-out source revision is a commit SHA"
    );
    value
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
#[test]
fn production_dispatch_uses_reviewed_task_target_and_build_receipts() {
    let executable_dir = tempfile::tempdir().unwrap();
    let executable = executable_dir.path().join("fixture-app");
    std::fs::write(&executable, b"synthetic desktop executable").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    let fixture =
        crate::app::root::test_runtime_qa::DesktopArtifactFixture::from_executable(&executable);
    let dispatch: crate::app::root::test_runtime_qa::RootlessDesktopDispatch =
        crate::app::root::test_runtime_qa::prepare_rootless_desktop_dispatch(
            &fixture.project,
            &fixture.task,
        )
        .unwrap();

    assert_eq!(
        dispatch.request.code_head_sha,
        fixture.task.reviewed_code_head_sha.as_deref().unwrap()
    );
    assert_eq!(
        dispatch.request.target_id.as_deref(),
        Some("qa-desktop-fixture-target")
    );
    assert_eq!(
        dispatch.target.executable,
        "/workspace/dist/approved-desktop-app"
    );
    assert_eq!(dispatch.target.arguments, ["--fixture-window".to_owned()]);
}
