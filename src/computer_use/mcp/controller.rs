//! App-owned implementation of provider-neutral Computer Use MCP operations.

mod cancellation;
mod dispatch;
mod session;
mod types;

pub use types::{ComputerUseMcpControllerV1, ComputerUseMcpRuntimeConfigV1};

#[cfg(test)]
#[path = "controller/tests.rs"]
mod tests;
