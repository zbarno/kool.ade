use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use super::super::mounts::{bind_readonly, bind_readonly_file, mount_tmpfs};
use super::path_safety::inside_workspace;

pub(super) fn mount_git_metadata(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    root: &Path,
    empty_file: &Path,
) -> anyhow::Result<PathBuf> {
    let git_entry = root.join(".git");
    let metadata = fs::symlink_metadata(&git_entry).map_err(|_| {
        anyhow::anyhow!("Implementation must run from Koolade's registered Git worktree")
    })?;
    anyhow::ensure!(
        metadata.is_file(),
        "Implementation must run from Koolade's registered Git worktree"
    );
    let admin = git_path(root, "--git-dir")?;
    let common = git_path(root, "--git-common-dir")?;
    validate_koolade_worktree(root, &admin, &common)?;
    bind_readonly_file(args, created, &git_entry, &git_entry);
    bind_readonly(args, created, &common, &common)?;
    if admin != common && !admin.starts_with(&common) {
        bind_readonly(args, created, &admin, &admin)?;
    }
    let worktrees = common.join("worktrees");
    if fs::symlink_metadata(&worktrees).is_ok_and(|metadata| metadata.is_dir()) {
        mount_tmpfs(args, created, &worktrees, 67_108_864);
        bind_readonly(args, created, &admin, &admin)?;
    }
    let diagnostics = common.join("koolade-harness");
    match fs::symlink_metadata(&diagnostics) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir(),
            "Harness diagnostics directory must not be a symlink or regular file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&diagnostics)?;
        }
        Err(error) => return Err(error.into()),
    }
    mount_tmpfs(args, created, &diagnostics, 67_108_864);
    mask_git_config(args, created, empty_file, &common.join("config"))?;
    if admin != common {
        mask_git_config(args, created, empty_file, &admin.join("config.worktree"))?;
    }
    Ok(common)
}

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

pub(in crate::harness::pi_sandbox) fn git_path(
    root: &Path,
    option: &str,
) -> anyhow::Result<PathBuf> {
    let output = Command::new(locate_git(root)?)
        .args(["rev-parse", "--path-format=absolute", option])
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot safely locate this worktree's Git metadata"
    );
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let path = path.canonicalize()?;
    anyhow::ensure!(path.is_dir(), "Git metadata path is not a directory");
    Ok(path)
}

pub(in crate::harness::pi_sandbox) fn locate_git(root: &Path) -> anyhow::Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        let candidate = directory.join("git");
        if !is_executable(&candidate) {
            continue;
        }
        let Ok(candidate) = candidate.canonicalize() else {
            continue;
        };
        if inside_workspace(&candidate, root) {
            continue;
        }
        return Ok(candidate);
    }
    anyhow::bail!("Cannot locate a trusted Git executable outside the task workspace")
}

fn mask_git_config(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    empty_file: &Path,
    path: &Path,
) -> anyhow::Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    anyhow::ensure!(
        metadata.is_file(),
        "Git configuration must be a regular file inside the sandbox"
    );
    bind_readonly_file(args, created, empty_file, path);
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}
