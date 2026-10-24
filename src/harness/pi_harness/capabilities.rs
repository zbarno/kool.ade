//! Runtime capability checks for the Pi CLI interface Koolade relies on.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};

use crate::error::AppError;

use super::super::{ExecutionMode, ToolAccess};

type HelpResult = Result<String, String>;
type HelpCache = HashMap<PathBuf, (Option<SystemTime>, HelpResult)>;

static HELP_CACHE: OnceLock<Mutex<HelpCache>> = OnceLock::new();

/// The settings/setup probe checks the complete interface Koolade uses.
pub(super) fn validate_all(exe: &Path) -> Result<(), AppError> {
    let help = help_output(exe).map_err(AppError::Other)?;
    let missing = ExecutionMode::ALL
        .into_iter()
        .flat_map(|mode| missing_capabilities(&help, mode))
        .collect::<std::collections::BTreeSet<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Pi is missing Koolade-required capabilities: {}",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        )))
    }
}

fn help_output(exe: &Path) -> Result<String, String> {
    let modified = exe.metadata().and_then(|metadata| metadata.modified()).ok();
    let cache = HELP_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((cached_modified, result)) = cache.get(exe)
        && *cached_modified == modified
    {
        return result.clone();
    }
    let result = read_help(exe);
    cache.insert(exe.to_owned(), (modified, result.clone()));
    result
}

fn read_help(exe: &Path) -> Result<String, String> {
    let output = crate::harness::discovery::run(
        &[exe.to_string_lossy().into_owned(), "--help".into()],
        Duration::from_secs(10),
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        &["NODE_OPTIONS"],
    )
    .map_err(|error| format!("could not inspect Pi capabilities: {error}"))?;
    if !output.success {
        return Err("Pi --help exited unsuccessfully".into());
    }
    let mut combined = output.stdout;
    combined.push(b'\n');
    combined.extend_from_slice(&output.stderr);
    String::from_utf8(combined).map_err(|_| "Pi --help returned non-UTF-8 output".into())
}

fn missing_capabilities(help: &str, mode: ExecutionMode) -> Vec<String> {
    let mut required = vec![
        "--print",
        "--mode",
        "--no-session",
        "--no-approve",
        "--append-system-prompt",
        "--thinking",
        "--provider",
        "--model",
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
    ];
    match mode.tool_access() {
        ToolAccess::None => required.push("--no-tools"),
        ToolAccess::ReadOnly => {
            required.extend(["--no-builtin-tools", "--tools", "--extension"]);
        }
        ToolAccess::BoundedImplementation => {
            required.extend(["--no-builtin-tools", "--tools", "--extension"]);
        }
        ToolAccess::RuntimeQaScreenshots => {
            required.extend(["--no-builtin-tools", "--tools", "--extension"]);
        }
    }
    let mut missing = required
        .into_iter()
        .filter(|flag| !has_option(help, flag))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !help.contains("json") {
        missing.push("JSON output mode".into());
    }
    if !help.contains("xhigh") {
        missing.push("xhigh thinking level".into());
    }
    missing.sort();
    missing.dedup();
    missing
}

fn has_option(help: &str, flag: &str) -> bool {
    help.lines().any(|line| {
        line.split_whitespace().next().is_some_and(|token| {
            token
                .split(',')
                .any(|option| option.trim_end_matches(',') == flag)
        })
    })
}

#[cfg(test)]
#[path = "capabilities/tests.rs"]
mod tests;
