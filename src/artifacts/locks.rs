//! Cross-process coordination gates for project state.
//!
//! Kool.ad/e processes coordinate first-use actions (such as generating task
//! evidence) with a file lock keyed to the Git common directory of the
//! repository they operate on. For managed planning state that repository is
//! the planning repository.
use std::{fs, path::PathBuf, process::Command};

pub(crate) fn common_dir(repo: &std::path::Path) -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot locate Git common directory for project state gate"
    );
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn root_identity(repo: &std::path::Path) -> anyhow::Result<String> {
    Ok(repo.canonicalize()?.to_string_lossy().into_owned())
}

fn root_key(repo: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(root_identity(repo)?.as_bytes());
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn lock_path(
    common: &std::path::Path,
    repo: &std::path::Path,
) -> anyhow::Result<std::path::PathBuf> {
    Ok(common.join(format!(
        "koolade-artifact-migration-{}.lock",
        root_key(repo)?
    )))
}

fn lock(common: &std::path::Path, repo: &std::path::Path) -> anyhow::Result<fs::File> {
    let path = lock_path(common, repo)?;
    fs::create_dir_all(common)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}

/// Coordinate first creation of task evidence and other project-wide state
/// transitions across Kool.ad/e instances sharing one repository.
pub(crate) fn acquire_project_state_gate(repo: &std::path::Path) -> anyhow::Result<fs::File> {
    lock(&common_dir(repo)?, repo)
}
