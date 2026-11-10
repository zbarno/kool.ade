mod action;
mod action_intent;
mod error;
mod event;
mod identity;
mod lease;
mod policy;
mod session;
mod state;
mod target;

pub use action::{
    ComputerActionActorV1, ComputerActionHelperAuthorizationV1, ComputerActionHelperPhaseV1,
    ComputerActionPayloadV1, ComputerActionV1, ComputerFrameReferenceV1, ComputerKeyV1,
    ComputerOperationV1, ComputerWaitConditionV1, ComputerWindowBoundsV1, KeyCodeV1, MouseButtonV1,
};
pub use action_intent::{
    ComputerActionApprovalV1, ComputerActionIntentStateV1, ComputerActionIntentV1,
    ComputerActionRiskV1,
};
pub use error::{ComputerFailureCodeV1, ComputerStatusV1};
pub use event::{
    ComputerEventKindV1, ComputerEventV1, ComputerEventValidator, ComputerObservationV1,
};
pub use identity::{ComputerIdentityV1, ComputerSessionClassV1};
pub use lease::ComputerLeaseV1;
pub use policy::{
    ComputerCapabilityV1, ComputerNetworkProfileV1, ComputerPolicyV1, ComputerResourceBudgetV1,
    MINIMUM_DISPOSABLE_VM_BUDGET_V1,
};
pub use session::{ComputerControllerProofV1, ComputerSessionV1};
pub use state::{ComputerLifecycleV1, can_transition};
pub use target::{
    COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1, ComputerHypervisorV1,
    ComputerRemoteWorkerTrustV1, ComputerTargetV1,
};

#[cfg(test)]
mod tests;

pub const COMPUTER_USE_SCHEMA_VERSION_V1: u16 = 1;
