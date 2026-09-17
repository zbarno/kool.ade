//! Packet — git-native LLM specification planner.
//!
//! See `planning/specification.md` for the product contract.
//! Module map (each file is intentionally small, ~<=300 lines):
//!
//! - [`domain`] — value types: open items, stakeholders, current user, chat log
//! - [`artifacts`] — planning file IO: specification.md, open-items.md, config.md, imports/
//! - [`persistence`] — out-of-git runtime state under `~/.packet` (chat history)
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
pub const PRODUCT_NAME: &str = "Packet";
pub const PRODUCT_TAGLINE: &str = "Git-native specification planner";

/// Prefix used for open-item identifiers (CLR-001, CLR-002, ...).
pub const ITEM_ID_PREFIX: &str = "CLR";
