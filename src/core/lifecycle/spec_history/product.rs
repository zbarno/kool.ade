use serde::{Deserialize, Serialize};

use super::{SpecDeltaClassificationV1, SpecReconciliationStatusV1};

/// One requirement's durable forward trace, with reverse-matchable references.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecRequirementTraceV1 {
    pub feature_uid: String,
    pub revision: u64,
    pub requirement_id: String,
    pub event_id: String,
    pub decision_refs: Vec<String>,
    pub task_refs: Vec<String>,
    pub test_refs: Vec<String>,
    pub pull_request_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecTraceReferenceKindV1 {
    LifecycleEvent,
    DecisionOrReceipt,
    Task,
    FollowupTask,
    Test,
    Evidence,
    PullRequestState,
    OperatorDecision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecTraceTargetKindV1 {
    LifecycleEvent,
    Decision,
    CodeReview,
    RuntimeQa,
    Task,
    TestReference,
    PullRequestState,
    OperatorDecision,
    EvidenceReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecTraceNavigationV1 {
    pub feature_uid: String,
    pub target_kind: SpecTraceTargetKindV1,
    pub target_uid: String,
    pub title: String,
    pub detail: String,
    pub requirement_traces: Vec<SpecRequirementTraceV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductSpecAggregateV1 {
    pub rendered_product_hash: String,
    pub status: ProductSpecAggregateStatusV1,
    pub lifecycle_features: Vec<ProductSpecFeatureSummaryV1>,
    pub findings: Vec<ProductSpecFindingV1>,
    pub pending_trigger_count: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductSpecAggregateStatusV1 {
    Synchronized,
    FindingsRequireReview,
    PendingTriggers,
    DriftDetected,
    BaselineIncomplete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductSpecFeatureSummaryV1 {
    pub feature_uid: String,
    pub revision: Option<u64>,
    pub approved_target_hash: Option<String>,
    pub reconciliation_status: Option<SpecReconciliationStatusV1>,
    pub drift_detected: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductSpecFindingV1 {
    pub feature_uid: String,
    pub revision: u64,
    pub requirement_id: String,
    pub event_id: String,
    pub decision_refs: Vec<String>,
    pub classification: SpecDeltaClassificationV1,
    pub evidence_refs: Vec<String>,
    pub test_refs: Vec<String>,
    pub affected_task_ids: Vec<String>,
    pub needs_user_reapproval: bool,
}
