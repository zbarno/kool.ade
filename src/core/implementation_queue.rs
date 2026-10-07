//! Durable Auto-mode preferences and dependency-aware queue selection.
mod scope_conflicts;
mod selection;
mod state;

pub use scope_conflicts::active_scope_conflict;
pub use selection::{
    next_ready_ticket, next_ready_ticket_with_running_scopes, next_ticket, ticket_readiness,
};
pub use state::Queue;
#[cfg(test)]
use state::directory;

#[cfg(test)]
#[path = "implementation_queue/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "implementation_queue/identity_tests.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "implementation_queue/parallel_features_tests.rs"]
mod parallel_features_tests;
