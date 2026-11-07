mod action;
mod error;
mod event;
mod identity;
mod lease;
mod policy;
mod session;
mod state;

pub use action::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1, ComputerOperationV1,
    KeyCodeV1, MouseButtonV1,
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
pub use session::{ComputerControllerProofV1, ComputerSessionV1, ComputerTargetV1};
pub use state::{ComputerLifecycleV1, can_transition};

#[cfg(test)]
mod tests;

pub const COMPUTER_USE_SCHEMA_VERSION_V1: u16 = 1;
