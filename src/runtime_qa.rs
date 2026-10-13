//! Versioned, evidence-backed contracts for independent runtime QA.

mod browser_driver;
mod contracts;
pub(crate) mod drivers;
mod gate;
mod orchestrator;
mod recipe_queue_handle;
mod store;
#[cfg(test)]
mod tests;
mod worker_handle;

pub use contracts::*;
pub use gate::approved_gate_receipt;
pub use orchestrator::{
    QaExecutionContext, QaReviewer, QaScenarioDriver, QaTargetAuthorization, RuntimeQaOrchestrator,
};
pub(crate) use recipe_queue_handle::RuntimeRecipeWorkerHandle;
pub use store::RuntimeQaStore;
pub(crate) use worker_handle::QaWorkerHandle;

/// App-owned line-independent protocol entrypoint for the pinned Linux
/// desktop QA driver image. This is exposed for its dedicated helper binary.
#[cfg(target_os = "linux")]
pub fn execute_linux_desktop_driver_request(bytes: &[u8]) -> Result<Vec<u8>, String> {
    drivers::desktop::execute_request_json(bytes)
}

/// Verify the private Xvfb display and screenshot path in the pinned image.
#[cfg(target_os = "linux")]
pub fn smoke_linux_desktop_driver() -> Result<(u16, u16, usize), String> {
    drivers::desktop::smoke_display()
}

/// Exercise real X11 keyboard/mouse interactions and seeded assertion failure.
#[cfg(target_os = "linux")]
pub fn smoke_linux_desktop_fixture() -> Result<serde_json::Value, String> {
    drivers::desktop::smoke_fixture()
}

/// Launch the synthetic X11 window as a child of the isolated smoke driver.
#[cfg(target_os = "linux")]
pub fn run_linux_desktop_fixture_window() -> Result<(), String> {
    drivers::desktop::run_fixture_window()
}
