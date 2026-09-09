//! Shared repository/planning-artifact helpers.
//!
//! Named-file module layout:
//! * `artifacts.rs`     — path constants, read/write utilities
//! * `spec_doc.rs`      — `planning/specification.md` lifecycle
//! * `items_io.rs`      — `planning/open-items.md` (re)serialization
//! * `config_io.rs`     — `.planner/config.md` parsing/serialization
//! * `imports_io.rs`    — `planning/imports/` intake (§6)

pub mod config_io;
pub mod imports_io;
pub mod items_io;
pub mod spec_doc;

mod artifacts;

pub use artifacts::{
    atomic_write, read_utf8_lossy, repo_artifact, sanitize_basename,
    CONFIG_DIR, IMPORTS_DIR, MCP_CONFIG_FILE, OPEN_ITEMS_FILE, PLANNING_DIR, SPEC_FILE, CONFIG_FILE,
};

pub mod task_docs;
