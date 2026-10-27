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
    /// Transient probe errors; readiness tasks live in the persisted collection.
    pub setup_attention: Vec<crate::app::setup_attention::SetupIssue>,
    pub setup_tasks: Vec<crate::persistence::setup_tasks::LocalTask>,
    pub setup_checking: bool,
    pub eligible_item_ids: BTreeSet<String>,
    pub archived: BTreeSet<String>,
    pub cancelled: BTreeSet<String>,
    /// Persisted lifecycle phase by task-document path; rendering only.
    pub lifecycle_tasks: std::collections::BTreeMap<String, crate::core::lifecycle::TaskPhase>,
    pub lifecycle_pr_statuses:
        std::collections::BTreeMap<String, crate::core::lifecycle::PullRequestPreparationStatus>,
    /// Separate PR-preparation cards linked to their completed parent tasks.
    pub lifecycle_pr_work: Vec<LifecyclePrBoardTask>,
    pub lifecycle_features: Vec<crate::core::lifecycle::FeatureGoal>,
    pub lifecycle_feature_tasks: Vec<crate::core::lifecycle::TaskRecord>,
    pub lifecycle_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct BootstrapConversationView {
    pub messages: Vec<crate::domain::chatlog::ChatMessage>,
    pub busy: bool,
    pub active_for_task: bool,
    pub error: Option<String>,
    pub suggestion: Option<BootstrapSuggestionView>,
}

#[derive(Clone)]
pub struct BootstrapSuggestionView {
    pub operation: BootstrapSuggestionOperation,
    pub turn_generation: u64,
}

#[derive(Clone)]
pub enum BootstrapSuggestionOperation {
    RecheckPrerequisites,
    PrepareLockedImage { profile: String },
    OpenCodingTools,
}

#[derive(Clone)]
pub struct LifecyclePrBoardTask {
    pub parent_path: String,
    pub parent_title: String,
    pub preparation: crate::core::lifecycle::PullRequestPreparation,
}

impl ViewModel {
    pub fn is_archived(&self, key: &str) -> bool {
        self.archived.contains(key)
    }
}
