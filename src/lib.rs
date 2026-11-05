//! Koolade — conversational project planner.
//!
//! See `docs/koolade/product/index.md` for the current product contract.
//! Module map (each file is intentionally small, ~<=300 lines):
//!
//! - [`domain`] — value types: open items, local user identity, chat log
//! - [`artifacts`] — published specification reads and private local task records
//! - [`persistence`] — out-of-git runtime state under `~/.koolade-packet`
//! - [`harness`] — [`AiHarness`] trait + pi-CLI implementation (external LLM boundary)
//! - [`core`] — planning engine: state, routing, validation, apply, git, turn pipeline
//! - [`app`] — desktop UI session wiring (eframe)

pub mod app;
pub mod artifacts;
pub mod computer_use;
pub mod core;
pub mod domain;
pub mod error;
pub mod execution;
pub mod harness;
pub mod persistence;
pub mod project_runtime;
pub mod project_runtime_capability;
pub mod runtime_qa;
pub mod ui;
pub mod windows_setup_helper;

#[cfg(test)]
pub(crate) mod test_support;

pub use error::AppError;

/// Human-readable product name shown in titles and chrome.
pub const PRODUCT_NAME: &str = "Kool.ad/e";
pub const PRODUCT_TAGLINE: &str = "Project planning, made clear";

/// Prefix used for open-item identifiers (CLR-001, CLR-002, ...).
pub const ITEM_ID_PREFIX: &str = "CLR";

mod build_identity;
/// Local panic and startup error reports.
pub mod diagnostics;
