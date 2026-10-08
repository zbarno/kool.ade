use std::path::Path;

pub(in crate::harness::pi_sandbox) fn validate_koolade_worktree(
    root: &Path,
    admin: &Path,
    common: &Path,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        admin != common && admin.starts_with(common.join("worktrees")),
        "Implementation worktree is not registered under its repository Git metadata"
    );
    anyhow::ensure!(
        common.file_name().is_some_and(|name| name == ".git"),
        "Implementation worktree has an unexpected Git metadata location"
    );
    let repository = common
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Git metadata has no repository parent"))?;
    let workspace_root = repository
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Repository has no workspace parent"))?;
    let project_directory = root
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Worktree has no project directory"))?;
    let worktree_registry = project_directory
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Worktree has no Koolade registry"))?;
    let expected_slug = crate::persistence::project_slug(repository);
    anyhow::ensure!(
        worktree_registry
            .file_name()
            .is_some_and(|name| name == ".koolade-worktrees")
            && worktree_registry.parent() == Some(workspace_root)
            && project_directory
                .file_name()
                .is_some_and(|name| name == std::ffi::OsStr::new(&expected_slug)),
        "Implementation worktree is outside Koolade's isolated worktree directory"
    );
    Ok(())
}
