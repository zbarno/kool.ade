//! Private Pi event logs live in Git metadata, outside project artifacts.
use crate::{error::AppError, harness::ExecutionMode};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) fn open(
    repo_root: &Path,
    mode: ExecutionMode,
    implementation_common: Option<&Path>,
) -> Result<Option<(PathBuf, fs::File)>, AppError> {
    let common = match implementation_common {
        Some(common) => common.to_path_buf(),
        None => match planning_common(repo_root) {
            Some(common) => common,
            None => return Ok(None),
        },
    };
    let directory = common.join("packet-harness");
    if let Err(error) = fs::create_dir_all(&directory) {
        if mode != ExecutionMode::Implementation {
            return Ok(None);
        }
        return Err(AppError::Other(format!(
            "Cannot create harness diagnostics: {error}"
        )));
    }
    let path = directory.join(format!(
        "{}-{}-events.jsonl",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    ));
    match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
    {
        Ok(file) => Ok(Some((path, file))),
        Err(_) if mode != ExecutionMode::Implementation => Ok(None),
        Err(error) => Err(AppError::Other(format!(
            "Cannot open harness diagnostics: {error}"
        ))),
    }
}

fn planning_common(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    path.is_dir().then_some(path)
}
