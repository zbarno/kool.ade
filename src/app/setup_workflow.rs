//! The "Prepare this computer" coordination layer (issue 554).
//!
//! A deterministic, provider-free orchestration loop that orders machine
//! setup work, reuses the existing scoped-consent repair controllers, and
//! keeps an auditable position across restarts. It never executes host
//! mutations itself: every operation still travels through its existing
//! typed review, approval binding, and verification evidence.
//!
//! Safety posture:
//! - The auto-eligible operation registry is intentionally empty; every
//!   repairing operation passes through an explicit consent decision.
//! - The driver is a pure function of an injected input snapshot, so the
//!   full lifecycle (ordering, pauses, declines, retries, halts) is testable
//!   without a machine, network, or AI provider.

mod driver;
mod order;
mod policy;
pub mod routes;

pub use driver::DEFAULT_RESCAN_COOLDOWN_MS;
pub use driver::{BeatInput, FlowAction, SlotFamily, SlotPhase, SlotStatus, beat};
pub use order::{SequenceError, sequence};
pub use policy::{Management, RETRY_LIMIT, classify, failed_attempt_count};
pub use routes::{FixRoute, can_decide_fix, can_request_fix, fix_route};
