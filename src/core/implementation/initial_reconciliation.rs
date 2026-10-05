mod cache;
mod prepare;
mod prompt;
pub(super) mod support;

use super::*;
use serde::{Deserialize, Serialize};
use support::{read_plan, write_plan};

const PLAN_FILE: &str = "base-reconciliation.json";
const MAX_ATTEMPTS: usize = 3;
pub(super) const CONTRACT: &str = "## Acceptance criteria\n- Local and fetched shared changes are both preserved in the reconciled starting point.\n- Repository-required baseline checks pass on the combined result.\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub(super) schema_version: u8,
    pub(super) base: String,
    pub(super) local_commit: String,
    pub(super) remote_commit: String,
    pub(super) common_base: String,
    pub(super) required_verification: Vec<String>,
    #[serde(default)]
    pub(super) verified_commit: Option<String>,
    #[serde(default)]
    pub(super) verification: Vec<String>,
}

pub(super) fn save_plan(
    dir: &Path,
    base: &str,
    local_commit: &str,
    remote_commit: &str,
    common_base: &str,
    required_verification: &[String],
) -> anyhow::Result<()> {
    let plan = Plan {
        schema_version: 3,
        base: base.into(),
        local_commit: local_commit.into(),
        remote_commit: remote_commit.into(),
        common_base: common_base.into(),
        required_verification: required_verification.to_vec(),
        verified_commit: None,
        verification: Vec::new(),
    };
    let path = dir.join(PLAN_FILE);
    if path.exists() {
        let existing = read_plan(&path)?;
        anyhow::ensure!(
            existing.base == plan.base
                && existing.local_commit == plan.local_commit
                && existing.remote_commit == plan.remote_commit
                && existing.common_base == plan.common_base,
            "A different starting-point reconciliation is already saved for this task"
        );
        return Ok(());
    }
    write_plan(&path, &plan)
}

pub(super) use self::prepare::prepare;

pub(super) fn load_plan(dir: &Path) -> anyhow::Result<Option<Plan>> {
    let path = dir.join(PLAN_FILE);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_plan(&path)?))
}

pub(super) fn pending_required_verification(dir: &Path) -> anyhow::Result<Vec<String>> {
    let Some(plan) = load_plan(dir)? else {
        return Ok(Vec::new());
    };
    Ok(plan
        .required_verification
        .into_iter()
        .filter(|command| !plan.verification.contains(command))
        .collect())
}

pub(super) fn required_verification(dir: &Path) -> anyhow::Result<Vec<String>> {
    Ok(load_plan(dir)?
        .map(|plan| plan.required_verification)
        .unwrap_or_default())
}

pub(super) fn report_check_is_covered(required: &[String], reported: &str) -> bool {
    support::quality_checks::report_check_is_covered(required, reported)
}

pub(super) fn required_command_for_report<'a>(
    required: &'a [String],
    reported: &str,
) -> Option<&'a str> {
    support::quality_checks::required_command_for_report(required, reported)
}

pub(super) fn verified_report_covers(dir: &Path, required: &[String]) -> bool {
    if required.is_empty() {
        return true;
    }
    let Ok(report) = fs::read(dir.join("verified-report.json")) else {
        return false;
    };
    let Ok(report) = serde_json::from_slice::<serde_json::Value>(&report) else {
        return false;
    };
    let Some(commands) = report
        .get("verification")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    required.iter().all(|required| {
        commands
            .iter()
            .any(|command| command.as_str() == Some(required))
    })
}
