//! Presentation layer (egui). Views draw exclusively through the
//! [`Surface`] trait, so the app state machine (`crate::app`) stays
//! decoupled from pixels.

pub mod chat_pane;
pub(crate) mod decision_choice;
pub mod feature_approval;
pub mod items_pane;
pub mod layout;
pub mod markdown;
pub mod message_text;
pub mod overlays;
pub mod planning_board;
pub mod planning_repository;
pub mod reply_tail;
pub mod spec_viewer;
mod specification_status;
pub mod surface;
mod surface_trait;
pub mod task_activity;
pub mod task_chat;
pub mod task_checklist;
pub mod task_detail;
pub mod theme;
pub mod toast;

pub use chat_pane::Intent;
pub use layout::HeaderAction;
pub use planning_repository::{
    PlanningCodeRepositoryView, PlanningRepositoryCommand, PlanningRepositoryViewModel,
};
pub use specification_status::FeatureSpecificationStatus;
pub use surface_trait::{RepositoryChoice, Surface};
pub use toast::ToastQueue;

/// Typed actions emitted by views and handled by the application state
/// machine. UI code reads state through `Surface` and reports intent here.
pub enum ApplicationCommand {
    OpenSpecificationTrace {
        feature_uid: String,
        kind: crate::core::lifecycle::SpecTraceReferenceKindV1,
        reference: String,
    },
    CloseSpecificationTrace,
    DecideFeatureSpecDelta {
        feature_uid: String,
        requirement_id: String,
        expected_sequence: u64,
        adopt: bool,
    },
    PrepareTaskChat {
        key: String,
    },
    SendTaskReply {
        key: String,
    },
    SendImplementationDecision {
        key: String,
    },
    CancelTaskReply {
        key: String,
    },
    RetryTaskChatSave,
    RetrySetupCheck,
    PlanningRepository(PlanningRepositoryCommand),
    CreatePlanningTask {
        kind: crate::core::planning_work::WorkKind,
        description: String,
        parent_uid: Option<String>,
        source_branch: Option<String>,
        destination_branch: Option<String>,
        routing_overrides:
            std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
    },
    DrainTaskChatSaves,
    CancelTask,
    SetMaxParallelTasks {
        count: usize,
    },
    SetAutoPlan {
        enabled: bool,
    },
    SetAutoBuild {
        enabled: bool,
    },
    SetAutoPublish {
        enabled: bool,
    },
    SetRequireIndependentChecks {
        enabled: bool,
    },
    TakeOverStaleTaskClaim {
        ticket: String,
    },
    ArchiveTask {
        ticket: String,
    },
    ApprovePublication {
        ticket: String,
    },
    RequestPublicationChanges {
        ticket: String,
    },
    CancelWork {
        key: String,
    },
    ApproveReviewItem {
        id: String,
    },
    AnswerClarification {
        uid: String,
        expected_revision: u64,
        answer: String,
        selected_option: Option<String>,
    },
    DeferClarification {
        uid: String,
        expected_revision: u64,
        details: crate::core::planning::DeferredAssumption,
    },
    DecideOpportunity {
        uid: String,
        expected_revision: u64,
        choice: crate::harness::prompt_catalog::OpportunityChoice,
        explanation: Option<String>,
    },
    RetryOpportunityPlanning {
        uid: String,
        expected_revision: u64,
    },
    SetOpportunityDiscoverySettings {
        settings: crate::core::opportunity::OpportunityDiscoverySettingsV1,
        expected_revision: u64,
    },
    SnoozeOpportunity {
        uid: String,
        expected_revision: u64,
        days: u8,
    },
    ApproveFeature {
        id: String,
    },
    ApproveFeatureForBoard {
        id: String,
    },
    RequestFeatureChanges {
        id: String,
        candidate_hash: String,
    },
    GenerateTasksForFeature {
        work_key: String,
        feature_id: String,
    },
    CompareFeaturePlans {
        id: String,
    },
    ChooseFeaturePlan {
        id: String,
        plan_id: String,
    },
    DiscardFeaturePlans {
        id: String,
    },
    UserIntent(Intent),
    HeaderAction(HeaderAction),
    TaskDetail(task_detail::Command),
}
