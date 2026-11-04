//! Process-safe task execution fences stored under private local application state.
mod claim_lease;
mod handle;
mod model;
mod request;
#[cfg(test)]
mod tests;

pub(crate) use handle::ClaimLeaseHandle;
pub use model::{
    ClaimError, ClaimExecutionAttempt, ClaimExecutionAttemptStatus, ClaimLease, ClaimRequest,
};
