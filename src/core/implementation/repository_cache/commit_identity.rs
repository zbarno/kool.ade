use super::Runner;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const TASK_IDENTITY_FILE: &str = "git-identity.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitCommitIdentity {
    pub(crate) name: String,
    pub(crate) email: String,
}

impl GitCommitIdentity {
    pub(crate) fn from_repository(repository: &Path, runner: &Runner) -> anyhow::Result<Self> {
        let name = runner.git(repository, &["config", "--get", "--default=", "user.name"])?;
        let email = runner.git(repository, &["config", "--get", "--default=", "user.email"])?;
        anyhow::ensure!(
            !name.is_empty() && !email.is_empty(),
            "Configure Git user.name and user.email before starting task implementations"
        );
        Ok(Self { name, email })
    }
}

pub(crate) fn read_for_task(dir: &Path) -> anyhow::Result<GitCommitIdentity> {
    let path = dir.join(TASK_IDENTITY_FILE);
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("Saved task Git identity is missing; preserved for review")
        } else {
            error.into()
        }
    })?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Saved task Git identity is not a regular file"
    );
    let identity: Option<GitCommitIdentity> = serde_json::from_slice(&fs::read(path)?)?;
    identity.ok_or_else(|| {
        anyhow::anyhow!("Saved task Git identity is empty; configure Git user.name and user.email")
    })
}

pub(crate) fn load_or_capture(
    dir: &Path,
    repository: &Path,
    runner: &Runner,
) -> anyhow::Result<GitCommitIdentity> {
    let path = dir.join(TASK_IDENTITY_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Saved task Git identity is not a regular file"
            );
            let identity: Option<GitCommitIdentity> = serde_json::from_slice(&fs::read(&path)?)?;
            if let Some(identity) = identity {
                return Ok(identity);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let identity = GitCommitIdentity::from_repository(repository, runner)?;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&Some(&identity))?)?;
    Ok(identity)
}

pub(crate) fn save_for_task(dir: &Path, identity: &GitCommitIdentity) -> anyhow::Result<()> {
    let path = dir.join(TASK_IDENTITY_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            anyhow::ensure!(
                read_for_task(dir)? == *identity,
                "Saved task Git identity changed; preserved for review"
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&Some(identity))?)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn configure_clone(
    repository: &Path,
    identity: &GitCommitIdentity,
    preserve_existing: bool,
    runner: &Runner,
) -> anyhow::Result<()> {
    runner.git(
        repository,
        &["config", "--local", "user.useConfigOnly", "true"],
    )?;
    for (key, value) in [
        ("user.name", identity.name.as_str()),
        ("user.email", identity.email.as_str()),
    ] {
        let current = runner.git(
            repository,
            &["config", "--local", "--get", "--default=", key],
        )?;
        if !preserve_existing || current.is_empty() {
            runner.git(repository, &["config", "--local", key, value])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
