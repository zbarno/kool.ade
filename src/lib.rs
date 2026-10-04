//! Koolade — conversational project planner.
//!
//! See `.koolade-packet/planning/product/index.md` for the current product contract.
//! Module map (each file is intentionally small, ~<=300 lines):
//!
//! - [`domain`] — value types: open items, stakeholders, current user, chat log
//! - [`artifacts`] — planning file IO: product modules, open-items.md, project config, imports/
//! - [`persistence`] — out-of-git runtime state under `~/.koolade` (chat history)
//! - [`harness`] — [`AiHarness`] trait + pi-CLI implementation (external LLM boundary)
//! - [`core`] — planning engine: state, routing, validation, apply, git, turn pipeline
//! - [`app`] — desktop UI session wiring (eframe)

pub mod app;
pub mod artifacts;
pub mod core;
pub mod domain;
pub mod error;
pub mod harness;
pub mod persistence;
pub mod ui;

pub use error::AppError;

/// Human-readable product name shown in titles and chrome.
pub const PRODUCT_NAME: &str = "Kool.ad/e";
pub const PRODUCT_TAGLINE: &str = "Project planning, made clear";

/// Prefix used for open-item identifiers (CLR-001, CLR-002, ...).
pub const ITEM_ID_PREFIX: &str = "CLR";

/// Local panic and startup error reports.
pub mod diagnostics;
