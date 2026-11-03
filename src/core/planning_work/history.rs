use serde::{Deserialize, Serialize};

/// Append-only, shared TaskIntent history stored with its root Work record.
/// Entries hold visible state changes and links, never private transcripts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskIntentEventKindV1 {
    Created,
    GoalRevised,
    StatusChanged,
    WorkLinked,
    ExecutableTaskLinked,
    FollowUpCycleStarted,
    DecisionRecorded,
    ApprovalRecorded,
    PlanDecisionRecorded,
    ReviewRecorded,
    QualityAssuranceRecorded,
    SpecificationReconciliationRecorded,
    DeliveryOutcomeRecorded,
    Archived,
    Restored,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskIntentEventV1 {
    pub event_uid: String,
    pub kind: TaskIntentEventKindV1,
    pub recorded_at_ms: u64,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_uid: Option<String>,
    /// Structured, source-bound provenance. Older events remain readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<TaskIntentEventEvidenceV1>,
}

mod capture;
mod evidence;
pub use capture::capture_authoritative_events;
pub(super) use capture::clip_event_summary;
pub use evidence::{TaskIntentEventEvidenceV1, TaskIntentGateKindV1};
