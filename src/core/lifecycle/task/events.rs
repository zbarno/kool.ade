use serde::{Deserialize, Serialize};

use super::super::types::GateReceipt;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEvent {
    Start,
    ImplementationFinished {
        code_sha256: String,
    },
    /// Invalidate completion gates when application-controlled destination
    /// integration changes the exact parent head before PR publication.
    PublicationHeadChanged {
        code_sha256: String,
        head_sha: String,
    },
    CodeReview {
        receipt: GateReceipt,
        reviewed_head_sha: String,
        review_subject_sha256: String,
    },
    BeginRepair,
    RuntimeQaRequested(crate::runtime_qa::QaRunRequestV1),
    RuntimeQaBlocked {
        request_id: String,
        reason: String,
        result_sha256: Option<String>,
        evidence_manifest_sha256: Option<String>,
    },
    /// Persist an actionable blocker when no safe request can be constructed.
    RuntimeQaUnavailable {
        reason: String,
    },
    RuntimeQaCompleted {
        request_id: String,
        receipt: GateReceipt,
        result_sha256: String,
        evidence_manifest_sha256: String,
        reviewer_execution_id: String,
        reviewer_session_id: String,
    },
    /// Record a headless/otherwise non-visual target decision after a reviewer
    /// assessment and explicit Manager approval; this is not a QA pass.
    RuntimeQaNotApplicable {
        assessment: Box<crate::runtime_qa::QaApplicabilityAssessmentV1>,
        reviewer_receipt: Box<GateReceipt>,
        receipt: Box<GateReceipt>,
    },
    ReconcileSpec {
        receipt: GateReceipt,
        pr_work_item_uid: String,
    },
    /// Complete a refreshed parent after destination integration; retain its
    /// existing PR preparation work item instead of creating a duplicate.
    PublicationGatesRefreshed {
        receipt: GateReceipt,
    },
    SpecReconciliationBlocked {
        reason: String,
        proposal_sha256: String,
        spec_sha256: String,
        proposal_json: Option<String>,
    },
    SpecReconciliationRetry {
        current_spec_sha256: String,
    },
    /// Complete an owned subtask after its own review, QA, and spec checks.
    /// Subtasks are included in the parent task's eventual PR.
    SubtaskReconciled {
        receipt: GateReceipt,
    },
    Cancel,
}
