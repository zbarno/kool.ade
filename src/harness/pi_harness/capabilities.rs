//! Runtime capability checks for the Pi CLI interface Koolade relies on.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use crate::{error::AppError, harness::pi_proc::StreamEvt};

use super::super::{ExecutionMode, ToolAccess};

type HelpResult = Result<String, String>;
type HelpCache = HashMap<PathBuf, (Option<SystemTime>, HelpResult)>;

static HELP_CACHE: OnceLock<Mutex<HelpCache>> = OnceLock::new();

/// Refuse execution if the selected mode needs flags the installed CLI lacks.
pub(super) fn validate(exe: &Path, mode: ExecutionMode) -> Result<(), AppError> {
    let help = help_output(exe).map_err(AppError::Other)?;
    let missing = missing_capabilities(&help, mode);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Pi is missing Koolade-required capabilities: {}",
            missing.join(", ")
        )))
    }
}

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
    let task = crate::harness::pi_proc::spawn_with_input_env_excluding(
        &[exe.to_string_lossy().into_owned(), "--help".into()],
        Path::new("."),
        None,
        &[],
        &["NODE_OPTIONS"],
    )
    .map_err(|error| format!("could not inspect Pi capabilities: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = String::new();
    loop {
        if Instant::now() >= deadline {
            task.kill();
            let _ = task.settle(Duration::from_secs(1));
            return Err("Pi capability probe timed out while reading --help".into());
        }
        match task.poll_next(Duration::from_millis(100)) {
            Ok(StreamEvt::Stdout(line) | StreamEvt::Stderr(line)) => {
                output.push_str(&line);
                output.push('\n');
            }
            Ok(StreamEvt::Exited(true)) => return Ok(output),
            Ok(StreamEvt::Exited(false)) => {
                return Err("Pi --help exited unsuccessfully".into());
            }
            Err(crate::harness::pi_proc::PollState::Pending) => {}
            Err(crate::harness::pi_proc::PollState::Closed) => {
                return Err("Pi capability probe ended before --help completed".into());
            }
        }
    }
}

fn missing_capabilities(help: &str, mode: ExecutionMode) -> Vec<String> {
    let mut required = vec![
        "--print",
        "--mode",
        "--no-session",
        "--no-approve",
        "--append-system-prompt",
        "--thinking",
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
    ];
    if mode != ExecutionMode::Implementation {
        required.push("--no-context-files");
    }
    match mode.tool_access() {
        ToolAccess::None => required.push("--no-tools"),
        ToolAccess::ReadOnly => required.push("--tools"),
        ToolAccess::BoundedImplementation => {
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
