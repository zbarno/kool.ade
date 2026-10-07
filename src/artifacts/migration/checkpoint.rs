use std::{path::Path, process::Command};

pub(super) fn run(repo: &Path, paths: &[String]) -> anyhow::Result<()> {
    let mut checkpoint_paths = Vec::new();
    for path in paths {
        let tracked = crate::core::gitops::tracked_in_head(repo, path);
        if !repo.join(path).exists() && !tracked {
            continue;
        }
        if !tracked && is_ignored(repo, path)? {
            continue;
        }
        checkpoint_paths.push(path.clone());
    }
    if checkpoint_paths.is_empty() {
        return Ok(());
    }
    crate::core::gitops::commit(
        repo,
        "koolade: migrate project artifacts",
        &checkpoint_paths,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "Migrated artifacts are preserved, but the migration checkpoint failed: {error}"
        )
    })?;
    Ok(())
}

fn is_ignored(repo: &Path, path: &str) -> anyhow::Result<bool> {
    let status = Command::new("git")
        .args(["check-ignore", "--quiet", "--"])
        .arg(path)
        .current_dir(repo)
        .status()?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => anyhow::bail!("git check-ignore exited with status {code:?}"),
    }
}
