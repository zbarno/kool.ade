//! App-owned implementation of provider-neutral Computer Use MCP operations.

mod cancellation;
mod dispatch;
mod human_control;
mod human_input;
mod scheduler;
mod session;
mod session_journal;
pub(crate) use session::unix_time_ms;
mod types;

pub use types::{ComputerUseMcpControllerV1, ComputerUseMcpRuntimeConfigV1};

#[cfg(test)]
#[path = "controller/tests.rs"]
mod tests;
