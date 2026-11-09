//! Authenticated, bounded transport for the guest-resident Computer Use companion.

mod auth;
mod frame;
mod journal;
mod types;

#[cfg(target_os = "linux")]
pub mod agent;
#[cfg(target_os = "linux")]
mod host;

pub use auth::{GuestBootConfigV1, GuestHandshakeErrorV1};
pub use frame::{MAX_CONTROL_FRAME_BYTES, MAX_SCREENSHOT_PNG_BYTES};
#[cfg(target_os = "linux")]
pub use host::{
    AuthenticatedGuestChannelV1, AuthorizedComputerUseSessionV1, ComputerUseAuthorityRegistryV1,
    ComputerUsePermissionV1, GuestHostTransportV1,
};
pub use journal::{
    GuestActionJournalStatusV1, GuestActionJournalV1, GuestJournalErrorV1, GuestReplayDecisionV1,
};
pub use types::{
    GUEST_PROTOCOL_VERSION_V1, GuestActionOutcomeV1, GuestActionReplyV1, GuestCapabilitySetV1,
    GuestEnvelopeV1, GuestExitReasonV1, GuestMessageV1, GuestObservationV1, GuestOperationResultV1,
    GuestProtocolErrorV1, GuestSenderV1, GuestWindowObservationV1,
};

pub(crate) use types::GuestEnvelopeContextV1;

pub(crate) use auth::{AuthenticatedGuestEnvelopeV1, GuestChannelKeyV1};

#[cfg(all(test, unix))]
#[path = "guest_protocol/tests.rs"]
mod tests;
