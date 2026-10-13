//! Typed lifecycle records and transition guards for the Man.ager workflow.

mod feature;
pub(crate) mod plan_snapshot;
mod spec_reconciliation;
mod store;
mod task;
mod types;

pub use feature::{FeatureEvent, FeatureGoal};
pub use plan_snapshot::{PlanRequirementV1, PlanSnapshotV1};
pub use spec_reconciliation::{SpecDeltaApprovalV1, SpecReconciliationBlockV1};
pub use store::task_transition::PullRequestPreparationStatus;
pub use store::{
    LifecycleActor, LifecycleEventKind, LifecycleEventRecord, LifecycleStore,
    PullRequestPreparation, PullRequestPublicationState,
};
pub use task::{QaExecutionStateV1, QaExecutionStatusV1, TaskEvent, TaskRecord};
pub use types::{FeaturePhase, GateActor, GateDecision, GateReceipt, TaskPhase};

#[cfg(test)]
#[path = "lifecycle/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lifecycle/test_support.rs"]
pub(crate) mod test_support;
