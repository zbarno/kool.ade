//! Shared repository/planning-artifact helpers.
//!
//! Named-file module layout:
//! * `artifacts.rs`     — path constants, read/write utilities
//! * `spec_doc.rs`      — current product-document lifecycle
//! * `items_io.rs`      — `.kool-ade-packet/planning/open-items.md` serialization
//! * `config_io.rs`     — `.kool-ade-packet/config/project.md` parsing/serialization
//! * `imports_io.rs`    — `.kool-ade-packet/planning/imports/` intake (§6)
//! * `mcp_io.rs`        — Packet MCP configuration load/save/clear/checkpoint (F-18)

pub mod config_io;
pub mod imports_io;
pub mod items_io;
pub mod layout;
pub mod mcp_io;
pub mod migration;
pub mod spec_doc;

mod atomic;
mod shared;

pub(crate) use atomic::sync_parent_directory;
pub use atomic::{atomic_copy_new, atomic_create_bytes, atomic_write, atomic_write_bytes};
pub use shared::{
    CONFIG_DIR, CONFIG_FILE, IMPORTS_DIR, MCP_CONFIG_FILE, OPEN_ITEMS_FILE, PLANNING_DIR,
    SPEC_FILE, read_utf8_lossy, repo_artifact, sanitize_basename,
};

pub mod task_docs;

pub mod product_docs;

pub mod packet;
pub mod transaction;
