//! Packet-authored alternatives shown before a feature is approved.

use serde::{Deserialize, Serialize};

/// One implementation candidate in a two-plan comparison.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanAlternative {
    pub id: String,
    pub objective: String,
    pub phases: Vec<String>,
    #[serde(alias = "files_touched")]
    pub files_touched: Vec<String>,
    #[serde(alias = "state_changes")]
    pub state_changes: Vec<String>,
    #[serde(alias = "failure_modes")]
    pub failure_modes: Vec<String>,
    #[serde(alias = "effort_band")]
    pub effort_band: String,
    #[serde(alias = "known_risks")]
    pub known_risks: Vec<String>,
    pub reversibility: String,
}

/// Advisory lean with repository evidence; the operator makes the choice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRecommendation {
    #[serde(alias = "plan_id")]
    pub plan_id: String,
    pub rationale: String,
    pub evidence: Vec<String>,
}
