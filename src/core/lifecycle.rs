//! Typed lifecycle records and transition guards for the Man.ager workflow.

mod feature;
mod plan_remediation;
pub(crate) mod plan_snapshot;
pub(crate) mod product_spec;
mod spec_history;
mod spec_reconciliation;
mod store;
mod task;
mod types;

pub use feature::{FeatureEvent, FeatureGoal};
pub use plan_remediation::{PlanFindingActionV1, PlanFindingDispositionV1, PlanRemediationV1};
pub use plan_snapshot::{PlanRequirementV1, PlanSnapshotV1};
pub use spec_history::{
    ProductSpecAggregateStatusV1, ProductSpecAggregateV1, ProductSpecFeatureSummaryV1,
    ProductSpecFindingV1, SpecDeltaApprovalRefV1, SpecDeltaClassificationV1,
    SpecDeltaOperatorChoiceV1, SpecDeltaOperatorDecisionV1, SpecDeltaRecordV1,
    SpecDeltaReferenceV1, SpecOriginDispositionV1, SpecOriginOutcomeV1, SpecReconciliationStatusV1,
    SpecRequirementTraceV1, SpecRevisionDraftV1, SpecRevisionV1, SpecSyncQueueEntryV1,
    SpecSyncQueueStatusV1, SpecSyncTriggerKindV1, SpecTraceNavigationV1, SpecTraceReferenceKindV1,
    SpecTraceTargetKindV1,
};
pub use spec_reconciliation::{
    FeatureDeltaAuthorizationV1, SpecDeltaApprovalV1, SpecReconciliationBlockV1,
};
pub(crate) use store::DeploymentObservation;
pub(crate) use store::Snapshot;
pub use store::task_transition::{
    PrPublicationScopeV1, PrPublicationTaskEvidenceV1, PullRequestPreparationStatus,
};
pub use store::{
    CurrentImplementationEvidenceV1, DeploymentEvidenceV1, LifecycleActor, LifecycleEventKind,
    LifecycleEventRecord, LifecycleStore, PullRequestPreparation, PullRequestPublicationState,
};
pub use task::{QaExecutionStateV1, QaExecutionStatusV1, TaskEvent, TaskRecord};
pub use types::{FeaturePhase, GateActor, GateDecision, GateReceipt, TaskPhase};

#[cfg(test)]
#[path = "lifecycle/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lifecycle/test_support.rs"]
pub(crate) mod test_support;
