//! Load/save/remove lifecycle for Koolade's MCP config (F-18, D-16).
//! The planner never interprets or receives the full file: context includes
//! configured server names only, withholding commands and secrets.
//!
//! The file lives in the workspace's operator-local state directory, OUTSIDE
//! every Git repository: MCP commands routinely carry credentials, so they
//! are never committed to code or shared planning repositories.
//! Validation remains syntactic (JSON well-formedness probe only) and
//! structural (blank ⇔ unconfigured).

use std::path::{Path, PathBuf};

use crate::error::AppError;

pub const MCP_FILE_NAME: &str = "mcp.json";

/// Dedicated checkpoint subject for a Write save (NFR-9: short imperative;
/// parallels the existing "settings: update stakeholders and identity").
pub const UPDATE_SUBJECT: &str = "settings: update mcp server configuration";
/// Dedicated checkpoint subject for a blank-save removal.
pub const CLEAR_SUBJECT: &str = "settings: clear mcp server configuration";

/// What a `load_state` call knows about the file on disk.
pub struct McpLoadState {
    /// The canonical MCP config exists as a regular file.
    pub present: bool,
    /// File contents (UTF-8), populated only when `present`.
    pub content: Option<String>,
    /// Human-readable read failure when the file exists but could not be
    /// read (permissions, non-UTF-8 bytes, …). Mutually exclusive with
    /// `content`.
    pub read_error: Option<String>,
}

/// Inspect the on-disk file without touching git. Reads are lossy-tolerant:
/// any IO failure degrades to `read_error`, never panics and never blocks.
pub fn load_state(local_config_dir: &Path) -> McpLoadState {
    let path = local_config_dir.join(MCP_FILE_NAME);
    let present = path.is_file();
    let mut content = None;
    let mut read_error = None;
    if present {
        match std::fs::read_to_string(&path) {
            Ok(text) => content = Some(text),
            Err(e) => read_error = Some(e.to_string()),
        }
    }
    McpLoadState {
        present,
        content,
        read_error,
    }
}

/// Syntax-only JSON well-formedness probe (NFR-5): the app deliberately
/// enforces NO server schema — the file's consumers sit outside the planner.
/// Any top-level JSON value (object, array, scalar, even `5`) passes.
pub fn probe_json(text: &str) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(text)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Which disk action a Save implies. Derived purely, so the dialog layer
/// and the tests share one rule table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSaveOp {
    /// No disk change warranted: no file & blank buffer, OR the buffer is
    /// BYTE-EXACTLY identical to the stored content (no normalization —
    /// cosmetic differences always rewrite).
    Unchanged,
    /// Write the buffer atomically and checkpoint under UPDATE_SUBJECT.
    Write,
    /// Remove the file (blank ⇒ unconfigured) and checkpoint under
    /// CLEAR_SUBJECT; also purges blank-residue files.
    Clear,
}

/// Four-way rule table for a Save. `current` is the on-disk content (only
/// meaningful when `file_present`).
pub fn classify(file_present: bool, current: Option<&str>, buffer: &str) -> McpSaveOp {
    let blank = buffer.trim().is_empty();
    if blank {
        if file_present {
            McpSaveOp::Clear
        } else {
            McpSaveOp::Unchanged
        }
    } else if file_present && current == Some(buffer) {
        McpSaveOp::Unchanged
    } else {
        McpSaveOp::Write
    }
}

/// Outcome of an `apply_save`, consumed by the dialog to pick toast,
/// feedback line, and keep-open behaviour.
#[derive(Debug)]
pub struct McpApplyReceipt {
    /// The operation actually performed.
    pub op: McpSaveOp,
    /// Short (7-char) checkpoint SHA, `None` for `Unchanged` (no commit).
    pub short_sha: Option<String>,
    /// JSON probe failure text when a `Write` landed on malformed input
    /// — informational only; the save proceeded (D-16 non-blocking rule).
    pub malformed: Option<String>,
}

/// Execute a Save against the operator-local config directory: classify,
/// then either no-op, atomic-write, or remove. The file is never committed
/// anywhere, so a save has no git effect and `short_sha` is always `None`.
pub fn apply_save(local_config_dir: &Path, buffer: &str) -> Result<McpApplyReceipt, AppError> {
    let _guard = crate::core::writer_gate::acquire();
    let st = load_state(local_config_dir);
    let op = classify(st.present, st.content.as_deref(), buffer);
    match op {
        McpSaveOp::Unchanged => Ok(McpApplyReceipt {
            op,
            short_sha: None,
            malformed: None,
        }),
        McpSaveOp::Write => {
            // Computed for the receipt, never fatal (D-16: warn, don't block).
            let malformed = probe_json(buffer).err();
            let prepared = prepare_dir(local_config_dir).map_err(|e| AppError::Io {
                op: "write mcp.json".to_string(),
                detail: e.to_string(),
            })?;
            crate::artifacts::atomic_write(&prepared, buffer).map_err(|e| AppError::Io {
                op: "write mcp.json".to_string(),
                detail: e.to_string(),
            })?;
            Ok(McpApplyReceipt {
                op,
                short_sha: None,
                malformed,
            })
        }
        McpSaveOp::Clear => {
            let path = local_config_dir.join(MCP_FILE_NAME);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(AppError::Io {
                        op: "remove mcp.json".to_string(),
                        detail: e.to_string(),
                    });
                }
            }
            Ok(McpApplyReceipt {
                op,
                short_sha: None,
                malformed: None,
            })
        }
    }
}

fn prepare_dir(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    Ok(dir.join(MCP_FILE_NAME))
}

#[cfg(test)]
#[path = "mcp_io/tests.rs"]
mod tests;
