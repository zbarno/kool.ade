use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    pub(crate) status: ReportStatus,
    pub(crate) blocker_disposition: BlockerDisposition,
    pub(crate) summary: String,
    pub(crate) acceptance_criteria: Vec<Criterion>,
    pub(crate) verification: Vec<String>,
    pub(crate) remaining: Vec<String>,
    #[serde(default)]
    pub(crate) human_choices: Vec<HumanChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HumanChoice {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) meaning: String,
    pub(crate) consequence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReportStatus {
    Complete,
    Blocked,
}

impl ReportStatus {
    pub(crate) fn wire_name(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BlockerDisposition {
    None,
    MachineRepair,
    HumanAction,
    EnvironmentPrerequisite,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Criterion {
    pub(crate) criterion: String,
    pub(crate) evidence: String,
}

pub(crate) fn parse_report(text: &str) -> anyhow::Result<Report> {
    let mut value: serde_json::Value = serde_json::from_str(text)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Implementation report must be a JSON object"))?;
    let version = match object
        .remove("schemaVersion")
        .or_else(|| object.remove("schema_version"))
    {
        None => 0,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Implementation report version must be an integer"))?,
    };
    anyhow::ensure!(
        version <= 2,
        "Unsupported implementation report version {version}"
    );
    if version == 0 {
        if object
            .get("blocker_disposition")
            .is_none_or(serde_json::Value::is_null)
        {
            let disposition = match object.get("status").and_then(serde_json::Value::as_str) {
                Some("complete") => "none",
                Some("blocked") => "human_action",
                _ => {
                    let report: Report = serde_json::from_value(value)?;
                    validate_human_choices(&report)?;
                    return Ok(report);
                }
            };
            object.insert(
                "blocker_disposition".into(),
                serde_json::Value::String(disposition.into()),
            );
        }
        if object
            .get("human_choices")
            .is_none_or(serde_json::Value::is_null)
        {
            object.insert("human_choices".into(), serde_json::Value::Array(Vec::new()));
        }
    }
    let report: Report = serde_json::from_value(value)?;
    validate_human_choices(&report)?;
    Ok(report)
}

pub(super) fn validate_report(report: &Report, ticket: &str) -> anyhow::Result<()> {
    validate_contract(report, ticket, true)
}

pub(super) fn validate_reconciliation_report(report: &Report, ticket: &str) -> anyhow::Result<()> {
    validate_contract(report, ticket, false)
}

fn validate_contract(report: &Report, ticket: &str, require_commands: bool) -> anyhow::Result<()> {
    validate_human_choices(report)?;
    anyhow::ensure!(
        report.status == ReportStatus::Complete
            && report.blocker_disposition == BlockerDisposition::None
            && report.remaining.is_empty(),
        "## Implementation blocked\n\n### What is complete\n\n{}\n\n### Next action(s)\n\n- {}",
        report.summary,
        if report.remaining.is_empty() {
            "Review the report and choose Resume implementation.".to_owned()
        } else {
            report.remaining.join("\n- ")
        }
    );
    anyhow::ensure!(
        !report.summary.trim().is_empty()
            && !report.acceptance_criteria.is_empty()
            && report
                .acceptance_criteria
                .iter()
                .all(|c| !c.criterion.trim().is_empty() && !c.evidence.trim().is_empty()),
        "Report lacks acceptance-criterion evidence"
    );
    let mut in_criteria = false;
    for line in ticket.lines() {
        if line.starts_with("## ") {
            in_criteria = line.trim().eq_ignore_ascii_case("## Acceptance criteria");
            continue;
        }
        if in_criteria && let Some(criterion) = line.trim().strip_prefix("- ") {
            anyhow::ensure!(
                report
                    .acceptance_criteria
                    .iter()
                    .any(|c| c.criterion.trim() == criterion.trim()),
                "Missing evidence for ticket acceptance criterion: {criterion}"
            );
        }
    }
    anyhow::ensure!(
        (!require_commands || !report.verification.is_empty())
            && report
                .verification
                .iter()
                .all(|c| !c.trim().is_empty() && !matches!(c.trim(), "true" | ":" | "exit 0")),
        "Report lacks meaningful verification commands"
    );
    Ok(())
}

fn validate_human_choices(report: &Report) -> anyhow::Result<()> {
    if report.human_choices.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        report.status == ReportStatus::Blocked
            && report.blocker_disposition == BlockerDisposition::HumanAction,
        "Human choices are only valid for a human-action blocker"
    );
    anyhow::ensure!(
        report.human_choices.len() >= 2,
        "A single human action belongs in remaining steps, not as a choice"
    );
    let mut ids = std::collections::BTreeSet::new();
    for choice in &report.human_choices {
        anyhow::ensure!(
            !choice.id.trim().is_empty()
                && choice.id.trim().len() <= 48
                && choice
                    .id
                    .bytes()
                    .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') })
                && ids.insert(choice.id.to_ascii_lowercase()),
            "Human choices need unique short IDs"
        );
        anyhow::ensure!(
            [&choice.label, &choice.meaning, &choice.consequence]
                .iter()
                .all(|part| !part.trim().is_empty() && part.chars().count() <= 600),
            "Human choices need a label, meaning, and consequence"
        );
    }
    Ok(())
}

pub(crate) fn response_contract() -> &'static str {
    "\n\nREPORT FORMAT: Include schemaVersion: 2 and blocker_disposition: `none` for complete, `machine_repair` for a code, test, or tool failure this worktree can repair, `human_action` for a decision that needs an operator, or `environment_prerequisite` when required host tools, dependencies, caches, or network access are unavailable to this sandbox. Environment prerequisites are not machine repairs: state the missing resource and the smallest action needed to provide it, then stop. Classify the actual cause, not the role name. Include the exact issue and consequence in summary. Return `human_choices` as an array of issue-specific `{id,label,meaning,consequence}` objects only when a person must choose between real alternatives; use an empty array for a single required action. Give each choice a unique short ID, explain what it means and what happens if chosen, and do not invent alternatives or impose a fixed option count. Put required human steps in `remaining`, separate from Kool.ad/e's follow-up.\n"
}

pub(crate) fn feasibility_preflight() -> &'static str {
    "FEASIBILITY PREFLIGHT — before edits or expensive checks, compare every requirement about an exact file list, commit footprint, history, or frozen baseline against the actual base commit and reachable history. If a requirement is already impossible because published commits contain forbidden changes, or would require rewriting history or out-of-scope files, stop and report blocked before implementation. Name the exact conflicting requirement and show the smallest concrete evidence. Derive any decision options only from this task's evidence; do not reuse remedies or assumptions from unrelated tasks. If evidence supports only one human action, report a step instead of inventing choices. Do not spend recovery turns repeating checks that cannot change this fact. Distinguish this from a code or test defect that can be repaired in this worktree.\n\n"
}

pub(super) fn external_blocker(report: &Report) -> bool {
    report.status == ReportStatus::Blocked
        && matches!(
            report.blocker_disposition,
            BlockerDisposition::HumanAction | BlockerDisposition::EnvironmentPrerequisite
        )
}

pub(super) fn external_blocker_detail(report: &Report, report_path: &Path) -> String {
    let choices = report
        .human_choices
        .iter()
        .map(|choice| {
            format!(
                "- ({}) {}\n  Means: {}\n  If chosen: {}",
                choice.id, choice.label, choice.meaning, choice.consequence
            )
        })
        .collect::<Vec<_>>();
    let choices = if choices.is_empty() {
        String::new()
    } else {
        format!("\n\n### Options\n\n{}", choices.join("\n"))
    };
    if report.blocker_disposition == BlockerDisposition::EnvironmentPrerequisite {
        format!(
            "## Waiting for environment\n\n{}{}\n\n### Next action(s)\n\n- {}\n\nFull report: {}\n\nProvision the required tools or dependencies, then resume implementation.",
            report.summary,
            choices,
            report.remaining.join("\n- "),
            report_path.display()
        )
    } else {
        format!(
            "## Waiting for user action\n\n{}{}\n\n### Next action(s)\n\n- {}\n\nFull report: {}\n\nResume implementation after these actions are complete.",
            report.summary,
            choices,
            report.remaining.join("\n- "),
            report_path.display()
        )
    }
}

#[cfg(test)]
#[path = "report/tests.rs"]
mod tests;
