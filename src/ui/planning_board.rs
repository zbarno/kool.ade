//! Read model for the planning and task board, assembled by the application
//! before rendering so the board does not query unrelated project state.

use std::collections::BTreeSet;

#[derive(Clone, Default)]
pub struct ViewModel {
    pub task_documents: Vec<crate::artifacts::task_docs::TaskDocument>,
    pub planning_work: Vec<crate::core::planning_work::Work>,
    pub planning_items: Vec<crate::domain::item::OpenItem>,
    pub clarification_questions: Vec<crate::core::planning::QuestionV1>,
    pub clarification_error: Option<String>,
    pub opportunities: Vec<(crate::core::opportunity::OpportunityV1, u64)>,
    pub opportunity_decisions:
        std::collections::BTreeMap<String, crate::core::opportunity::OpportunityDecisionV1>,
    pub self_improvement_runs:
        std::collections::BTreeMap<String, crate::core::self_improvement::SelfImprovementRunV1>,
    pub opportunity_error: Option<String>,
    pub opportunity_discovery_settings: crate::core::opportunity::OpportunityDiscoverySettingsV1,
    pub setup_attention: Option<crate::app::setup_attention::SetupIssue>,
    pub setup_checking: bool,
    pub eligible_item_ids: BTreeSet<String>,
    pub archived: BTreeSet<String>,
    pub cancelled: BTreeSet<String>,
    /// Persisted lifecycle phase by task-document path; rendering only.
    pub lifecycle_tasks: std::collections::BTreeMap<String, crate::core::lifecycle::TaskPhase>,
    pub lifecycle_pr_statuses:
        std::collections::BTreeMap<String, crate::core::lifecycle::PullRequestPreparationStatus>,
    pub lifecycle_features: Vec<crate::core::lifecycle::FeatureGoal>,
    pub lifecycle_feature_tasks: Vec<crate::core::lifecycle::TaskRecord>,
    pub lifecycle_error: Option<String>,
}

impl ViewModel {
    pub fn is_archived(&self, key: &str) -> bool {
        self.archived.contains(key)
    }
}
