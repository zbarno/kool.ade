//! Durable Auto-mode preferences and dependency-aware queue selection.
mod selection;
mod state;

pub use selection::{next_ready_ticket, next_ticket, ticket_readiness};
pub use state::Queue;
#[cfg(test)]
use state::directory;

#[cfg(test)]
#[path = "implementation_queue/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "implementation_queue/identity_tests.rs"]
mod identity_tests;
