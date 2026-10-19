use std::path::Path;

use bollard::models::{Mount, MountBindOptions, MountType};

use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

pub(super) fn bind_mount(
    source: &Path,
    target: &str,
    read_only: bool,
    create_mountpoint: bool,
) -> Result<Mount, ExecutionError> {
    let source = source
        .to_str()
        .ok_or_else(|| denied("Task clone path is not valid UTF-8"))?;
    Ok(Mount {
        target: Some(target.into()),
        source: Some(source.into()),
        typ: Some(MountType::BIND),
        read_only: Some(read_only),
        bind_options: Some(MountBindOptions {
            propagation: Some(bollard::models::MountBindOptionsPropagationEnum::RPRIVATE),
            non_recursive: Some(true),
            create_mountpoint: Some(create_mountpoint),
            ..Default::default()
        }),
        ..Default::default()
    })
}

pub(super) fn validate_root(path: &Path) -> Result<(), ExecutionError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| denied("Private-file staging directory is unavailable"))?;
    let canonical = path
        .canonicalize()
        .map_err(|_| denied("Private-file staging directory is unavailable"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || canonical != path {
        return Err(denied("Private-file staging directory is unsafe"));
    }
    Ok(())
}

fn denied(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}
