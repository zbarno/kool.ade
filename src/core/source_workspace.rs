//! Bind repository tools to the selected project and an immutable source revision.
pub(crate) fn bind(
    state: &crate::core::state::PlannerState,
    task_id: &str,
) -> Option<crate::execution::contract::WorkspaceRef> {
    let repo_root = &state.repo_root;
    let metadata = std::fs::symlink_metadata(repo_root).ok()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    let metadata = std::fs::symlink_metadata(repo_root.join(".git")).ok()?;
    if metadata.file_type().is_symlink() || !(metadata.is_dir() || metadata.is_file()) {
        return None;
    }
    let root = repo_root.canonicalize().ok()?;
    let mut git = crate::core::planning_sync::validated_git_command(&root).ok()?;
    let output = git.args(["rev-parse", "--verify", "HEAD"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let source_sha = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    if !crate::execution::contract::is_valid_sha(&source_sha) {
        return None;
    }
    let workspace = crate::execution::contract::WorkspaceRef {
        project_id: state.planning_store.project_id.hyphenated().to_string(),
        task_id: task_id.to_owned(),
        source_sha,
    };
    workspace.validate().ok()?;
    Some(workspace)
}

/// A newly initialized repository has no source snapshot to inspect yet.
/// It may still support context-only planning without repository tools.
pub(crate) fn is_unborn(state: &crate::core::state::PlannerState) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(state.repo_root.join(".git")) else {
        return false;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(mut git) = crate::core::planning_sync::validated_git_command(&state.repo_root) else {
        return false;
    };
    let Ok(output) = git.args(["symbolic-ref", "--quiet", "HEAD"]).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let Ok(reference) = String::from_utf8(output.stdout) else {
        return false;
    };
    let reference = reference.trim();
    if !reference.starts_with("refs/heads/") {
        return false;
    }
    let Ok(mut git) = crate::core::planning_sync::validated_git_command(&state.repo_root) else {
        return false;
    };
    git.args(["show-ref", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.code() == Some(1))
}
