use std::{fs, path::Path};

use super::create_private_dir;
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

pub(super) fn copy_workspace_snapshot(
    manifest: &crate::execution::remote::WorkspaceManifestV1,
    source: &Path,
    destination: &Path,
) -> anyhow::Result<()> {
    manifest.validate()?;
    create_private_dir(destination)?;
    for entry in &manifest.entries {
        if entry.kind == crate::execution::remote::WorkspaceEntryKindV1::Directory {
            create_private_dir(&destination.join(&entry.path))?;
        }
    }
    for entry in &manifest.entries {
        if entry.kind == crate::execution::remote::WorkspaceEntryKindV1::File {
            let target = destination.join(&entry.path);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            manifest
                .stream_file(source, &entry.path, |bytes| {
                    use std::io::Write;
                    file.write_all(bytes).map_err(|_| {
                        ExecutionError::new(
                            ExecutionErrorKind::WorkerDisconnected,
                            "Verified workspace file could not be materialized",
                            false,
                        )
                    })
                })
                .map_err(|_| {
                    anyhow::anyhow!("workspace snapshot changed during materialization")
                })?;
            file.sync_all()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &target,
                    fs::Permissions::from_mode(if entry.executable { 0o700 } else { 0o600 }),
                )?;
            }
        }
    }
    #[cfg(not(unix))]
    anyhow::ensure!(
        !manifest
            .entries
            .iter()
            .any(|entry| entry.kind == crate::execution::remote::WorkspaceEntryKindV1::Symlink),
        "worker symlink materialization is unsupported"
    );
    #[cfg(unix)]
    for entry in &manifest.entries {
        if entry.kind == crate::execution::remote::WorkspaceEntryKindV1::Symlink {
            let target = destination.join(&entry.path);
            let target_value = entry
                .symlink_target
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("workspace symlink target is missing"))?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target_value, target)?;
        }
    }
    Ok(())
}

pub(super) fn initialize_isolated_git(root: &Path, staging_root: &Path) -> anyhow::Result<()> {
    let template = staging_root.join("git-template");
    create_private_dir(&template)?;
    let global = template.join("gitconfig");
    fs::write(&global, b"")?;
    let init = git(root, &global)
        .args(["init", "--quiet", "--initial-branch=main"])
        .arg(format!("--template={}", template.display()))
        .status()?;
    anyhow::ensure!(
        init.success(),
        "isolated worker Git repository could not be initialized"
    );
    let add = git(root, &global)
        .args([
            "-c",
            "user.name=Kool.ad/e Worker",
            "-c",
            "user.email=worker@kool.ad",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "add",
            "-A",
        ])
        .status()?;
    anyhow::ensure!(
        add.success(),
        "isolated worker snapshot could not be indexed"
    );
    let commit = git(root, &global)
        .args([
            "-c",
            "user.name=Kool.ad/e Worker",
            "-c",
            "user.email=worker@kool.ad",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "Verified remote input snapshot",
        ])
        .status()?;
    anyhow::ensure!(commit.success(), "isolated worker snapshot commit failed");
    Ok(())
}

fn git(root: &Path, global: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", global)
        .env("GIT_CONFIG_COUNT", "0")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .current_dir(root);
    command
}
