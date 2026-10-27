use std::time::Duration;

use super::super::process::TargetProcess;

pub(super) fn require_started(
    process: Option<&mut TargetProcess>,
    budget: Duration,
) -> Result<(), String> {
    let process =
        process.ok_or_else(|| "approved target process is unavailable after launch".to_owned())?;
    match process.startup_failure(budget)? {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub(super) fn exited_before_next_action(
    process: Option<&mut TargetProcess>,
    has_next_action: bool,
) -> bool {
    has_next_action && process.is_some_and(|child| child.is_running().ok() == Some(false))
}
