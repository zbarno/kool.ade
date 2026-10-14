//! Private Pi event logs live beside the isolated controller configuration.
use crate::{error::AppError, harness::ExecutionMode};
use std::{fs, path::PathBuf};

pub(super) fn open(
    controller_directory: &std::path::Path,
    mode: ExecutionMode,
) -> Result<Option<(PathBuf, fs::File)>, AppError> {
    let directory = controller_directory.join("koolade-harness");
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
