pub(super) mod cache;
mod clone_plan;
mod integration_state;
mod prepare;
mod prompt;
pub(super) mod support;

use super::*;
use serde::{Deserialize, Serialize};
use support::read_plan;

const PLAN_FILE: &str = "base-reconciliation.json";
const MAX_ATTEMPTS: usize = 3;
pub(super) const CONTRACT: &str = "## Acceptance criteria\n- Local and fetched shared changes are both preserved in the reconciled starting point.\n- Combined source is ready for application-owned repository baseline verification.\n";

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) clone_repository: Option<CloneRepositoryPlan>,
}

pub(super) use self::prepare::prepare;
#[cfg(test)]
pub(super) use clone_plan::save_plan;
pub(super) use clone_plan::{CloneRepositoryPlan, attach_clone_repository, save_clone_plan};
pub(super) use integration_state::{integrated_candidate_matches, mark_integrated_candidate};

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
