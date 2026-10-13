//! Tracks Kool.ad/e product improvements through the ordinary task lifecycle.
//! This record tracks PR/activation; it grants no execution or publication authority.

use serde::{Deserialize, Serialize};

mod run_validation;
mod store;
mod transition;
mod validation;
pub use store::{SelfImprovementStore, SelfImprovementTransitionV1};

#[cfg(test)]
#[path = "self_improvement/tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfImprovementEventV1 {
    BeginPlanning,
    PlanApproved {
        approval_sha256: String,
        review_uid: String,
        source_ref: String,
        destination_ref: String,
        task_uids: Vec<String>,
        owner_task_uid: String,
        #[serde(default)]
        parent_task_uids: Vec<String>,
    },
    ParentTaskProgress(SelfImprovementParentTaskProgressV1),
    CloneAllocated {
        clone_uid: String,
    },
    ImplementationFinished {
        head_sha: String,
        subject_sha256: String,
    },
    CodeReviewPassed {
        receipt_uid: String,
        subject_sha256: String,
    },
    QaPassed {
        receipt_uid: String,
        subject_sha256: String,
    },
    SpecReconciled {
        receipt_uid: String,
        subject_sha256: String,
    },
    PrPrepared {
        work_item_uid: String,
    },
    PrOpened {
        url: String,
    },
    Merged {
        sha: String,
    },
    Activated {
        version: String,
        readiness_probe_passed: bool,
    },
    NeedsAttention,
    Cancel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfImprovementPhaseV1 {
    ApprovedToPlan,
    Planning,
    Ready,
    Implementing,
    Reviewing,
    Qa,
    SpecSync,
    DoneAwaitingPr,
    PrPending,
    PrOpened,
    MergedNotActivated,
    Activated,
    NeedsAttention,
    Cancelled,
}

/// Projection of a regular Kool.ad/e feature/task run. Plan, code review,
/// runtime QA, spec sync and publication still use their existing app-owned gates.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelfImprovementRunV1 {
    pub schema_version: u16,
    pub run_uid: String,
    pub opportunity_uid: String,
    pub approved_decision_uid: String,
    pub target_repository_uid: String,
    pub canonical_remote: String,
    pub source_ref: Option<String>,
    pub source_sha: String,
    pub destination_ref: Option<String>,
    pub feature_uid: String,
    pub task_uids: Vec<String>,
    pub owner_task_uid: Option<String>,
    /// Durable progress for every standalone parent task. Older run records
    /// are read with an empty list and are backfilled by the app synchronizer.
    #[serde(default)]
    pub parent_task_progress: Vec<SelfImprovementParentTaskProgressV1>,
    pub clone_uid: Option<String>,
    pub plan_approval_hash: Option<String>,
    pub independent_plan_review_uid: Option<String>,
    pub implementation_head_sha: Option<String>,
    pub implementation_subject_sha256: Option<String>,
    pub code_review_uids: Vec<String>,
    pub runtime_qa_uids: Vec<String>,
    pub spec_receipt_uid: Option<String>,
    pub pr_work_item_uid: Option<String>,
    pub pr_url: Option<String>,
    pub merged_sha: Option<String>,
    pub activated_version: Option<String>,
    pub phase: SelfImprovementPhaseV1,
    pub revision: u64,
}

/// Evidence projection for one standalone task and its separately published
/// PR. Child subtasks remain represented by their parent lifecycle record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelfImprovementParentTaskProgressV1 {
    pub task_uid: String,
    pub phase: crate::core::lifecycle::TaskPhase,
    pub implementation_sha256: Option<String>,
    pub code_review_uid: Option<String>,
    pub runtime_qa_uid: Option<String>,
    pub spec_receipt_uid: Option<String>,
    pub pr_work_item_uid: Option<String>,
    pub pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    pub pr_url: Option<String>,
    pub pr_head_sha: Option<String>,
}
