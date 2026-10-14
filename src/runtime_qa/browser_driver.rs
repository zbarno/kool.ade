//! Request and report adapters for the pinned browser QA container.

mod base64;
mod fields;
mod report;
mod request;

pub use report::parse_browser_output;
pub use request::build_browser_request;

const MAX_DRIVER_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DRIVER_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
