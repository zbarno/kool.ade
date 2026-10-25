use std::{collections::HashSet, path::Path};

use crate::execution::errors::ExecutionError;

pub(super) fn verify_source_revision(root: &Path, expected: &str) -> Result<(), ExecutionError> {
    let root = root
        .to_str()
        .ok_or_else(|| super::denied("Task clone path is not valid UTF-8"))?;
    let output = run_git_bounded(
        vec![
            "git".into(),
            "-C".into(),
            root.into(),
            "-c".into(),
            "core.fsmonitor=false".into(),
            "rev-parse".into(),
            "--verify".into(),
            "HEAD^{commit}".into(),
        ],
        1024,
    )?;
    if !String::from_utf8_lossy(&output)
        .trim()
        .eq_ignore_ascii_case(expected)
    {
        return Err(super::denied(
            "Task clone HEAD does not match the pinned source SHA",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn tracked_executables(root: &Path) -> Result<HashSet<String>, ExecutionError> {
    let root = root
        .to_str()
        .ok_or_else(|| super::denied("Task clone path is not valid UTF-8"))?;
    let output = run_git_bounded(
        vec![
            "git".into(),
            "-C".into(),
            root.into(),
            "-c".into(),
            "core.fsmonitor=false".into(),
            "ls-files".into(),
            "--stage".into(),
            "-z".into(),
        ],
        super::super::manifest::MAX_WORKSPACE_MANIFEST_BYTES,
    )?;
    let mut executable_files = HashSet::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(separator) = record.iter().position(|byte| *byte == b'\t') else {
            return Err(super::denied("Git executable mode data is malformed"));
        };
        let (metadata, path) = record.split_at(separator);
        let path = &path[1..];
        if metadata.starts_with(b"100755 ") {
            let path = std::str::from_utf8(path)
                .map_err(|_| super::denied("Task clone contains a non-UTF8 path"))?;
            executable_files.insert(path.replace('\\', "/"));
        }
    }
    Ok(executable_files)
}

#[cfg(not(windows))]
pub(super) fn tracked_executables(_root: &Path) -> Result<HashSet<String>, ExecutionError> {
    Ok(HashSet::new())
}

fn run_git_bounded(argv: Vec<String>, stdout_limit: usize) -> Result<Vec<u8>, ExecutionError> {
    let excluded_environment = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_TRACE",
        "GIT_TRACE_SETUP",
    ];
    let output = crate::harness::discovery::run(
        &argv,
        std::time::Duration::from_secs(5),
        stdout_limit,
        16 * 1024,
        &excluded_environment,
    )
    .map_err(|_| super::denied("Git source metadata could not be verified"))?;
    if !output.success || output.stdout_truncated || output.stderr_truncated {
        return Err(super::denied("Git source metadata could not be verified"));
    }
    Ok(output.stdout)
}
