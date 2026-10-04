//! Interview readiness, explicit task-generation consent, and detailed story validation.
use crate::core::{state::PlannerState, validation::NormalizedTurn};
use crate::harness::TurnEnvelope;
use serde::{Deserialize, Serialize};

mod comparison_record;
#[cfg(test)]
#[path = "workflow/contract_tests.rs"]
mod contract_tests;
mod feature_approval;
mod outline_validation;
mod preparation;
mod story_validation;
pub use comparison_record::{PlanComparisonRecord, PlanComparisonStatus};
pub use outline_validation::validate_outline;
use story_validation::validate_stories;
pub use story_validation::{descriptive_title, story_detail_errors};

pub const WORKFLOW_FILE: &str = crate::artifacts::layout::canonical::WORKFLOW;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPurpose {
    #[default]
    Interview,
    /// Answer an investigative question without creating a feature spec.
    Question,
    /// Refresh a stale brief for an already authorized generation action.
    ReviewForGeneration,
    GenerateTasks,
    ComparePlans,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewBrief {
    #[serde(alias = "feature_name")]
    pub feature_name: String,
    pub problem: String,
    pub goal: String,
    #[serde(alias = "target_users")]
    pub target_users: String,
    #[serde(alias = "intended_outcome")]
    pub intended_outcome: String,
    #[serde(alias = "success_criteria")]
    pub success_criteria: Vec<String>,
    #[serde(alias = "in_scope")]
    pub in_scope: Vec<String>,
    #[serde(alias = "out_of_scope")]
    pub out_of_scope: Vec<String>,
    pub constraints: Vec<String>,
    #[serde(alias = "ready_for_tasks")]
    pub ready_for_tasks: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub brief: Option<InterviewBrief>,
    /// The exact specification the readiness assessment covers.
    pub reviewed_specification: Option<String>,
    pub task_batches: Vec<TaskBatchRef>,
    #[serde(default)]
    pub approved_features: std::collections::BTreeMap<String, String>,
    /// Authoritative plan comparison lifecycle, keyed by stable feature ID.
    #[serde(default)]
    pub plan_comparisons: std::collections::BTreeMap<String, PlanComparisonRecord>,
    /// Unvalidated pre-contract comparison snapshots kept as history only.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub legacy_plan_comparison_evidence:
        std::collections::BTreeMap<String, Vec<crate::domain::PlanComparison>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBatchRef {
    /// Koolade-owned immutable identity. Missing only in legacy workflow data.
    #[serde(default)]
    pub identity: Option<crate::domain::ArtifactIdentity>,
    pub feature: String,
    pub directory: String,
    pub count: usize,
}

fn default_repository() -> String {
    String::new()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskStory {
    pub title: String,
    #[serde(default = "default_repository", alias = "target_repository")]
    pub target_repository: String,
    pub intent: String,
    pub goal: String,
    pub context: String,
    #[serde(alias = "technical_design")]
    pub technical_design: Vec<String>,
    #[serde(alias = "verification_commands")]
    pub verification_commands: Vec<String>,
    #[serde(alias = "user_story")]
    pub user_story: String,
    pub purpose: String,
    /// One-based references into the approved brief, checked for full coverage.
    #[serde(alias = "scope_items")]
    pub scope_items: Vec<usize>,
    #[serde(alias = "success_criteria")]
    pub success_criteria: Vec<usize>,
    pub dependencies: Vec<usize>,
    #[serde(alias = "affected_files")]
    pub affected_files: Vec<String>,
    #[serde(alias = "implementation_steps")]
    pub implementation_steps: Vec<String>,
    #[serde(alias = "acceptance_criteria")]
    pub acceptance_criteria: Vec<String>,
    #[serde(alias = "test_plan")]
    pub test_plan: Vec<String>,
    #[serde(alias = "edge_cases")]
    pub edge_cases: Vec<String>,
    #[serde(alias = "rollout_notes")]
    pub rollout_notes: String,
    #[serde(alias = "definition_of_done")]
    pub definition_of_done: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskOutline {
    pub title: String,
    pub purpose: String,
    #[serde(alias = "target_repository")]
    pub target_repository: String,
    #[serde(alias = "scope_items")]
    pub scope_items: Vec<usize>,
    #[serde(alias = "success_criteria")]
    pub success_criteria: Vec<usize>,
    pub dependencies: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct TaskBatch {
    pub brief: InterviewBrief,
    pub specification: String,
    pub feature_id: Option<String>,
    pub contract: Option<crate::core::contract_snapshot::BatchContract>,
    pub stories: Vec<TaskStory>,
}

pub use feature_approval::{
    approve_feature, approve_feature_if_current, feature_approved, feature_contract,
};

impl Workflow {
    pub fn ready(&self, specification: Option<&str>) -> bool {
        self.brief.as_ref().is_some_and(|b| b.ready_for_tasks)
            && specification.is_some_and(|s| !s.trim().is_empty())
            && self.reviewed_specification.as_deref() == specification
    }
}

/// Scan `text` for stable `F<number>` and legacy `CHG-nnn` feature
/// identifiers, de-duplicated and sorted. Prose that embeds no id (legacy MVP
/// batches) yields an empty list and grandfathers through identity checks.
pub fn feature_ids_in(text: &str) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        if crate::artifacts::product_docs::valid_feature_id(token) {
            ids.insert(token.to_string());
        }
    }
    ids.into_iter().collect()
}

/// Judge a brief's declared feature identity (`declared`, de-duplicated)
/// against the feature the batch would actually be stamped with (`stamped`,
/// the active feature id). `feature_dir_exists` tests whether a candidate id
/// has a feature document. Returns an actionable operator message when the
/// two identities cannot both be honored; `None` means the guard passes.
pub fn brief_target_problem(
    declared: &[String],
    stamped: Option<&str>,
    feature_dir_exists: &dyn Fn(&str) -> bool,
) -> Option<String> {
    match declared {
        [] => None,
        [id] if !feature_dir_exists(id) => Some(format!(
            "Brief references unknown feature {id}; record the change specification under .koolade-packet/planning/changes before generating tasks"
        )),
        [id] => match stamped {
            Some(stamped) if *id == stamped => None,
            Some(stamped) => Some(format!(
                "Brief targets {id}, but the active feature is {stamped}; generation stamps every story with {stamped} and freezes its specification — make {id} the active feature (conclude {stamped} first, or reopen {id} if it was concluded) before generating its tasks"
            )),
            None => Some(format!(
                "Brief references feature {id}, but no feature is active"
            )),
        },
        _ => Some(format!(
            "Brief feature name declares more than one feature id ({}); name exactly one",
            declared.join(", ")
        )),
    }
}

pub use preparation::prepare;
use preparation::substantive;

#[cfg(test)]
#[path = "workflow/tests.rs"]
mod tests;
