//! Load/save/remove/checkpoint lifecycle for Koolade's MCP config (F-18,
//! D-16). The planner never interprets or receives the full file: context
//! includes configured server names only, withholding commands and secrets.
//! Validation remains syntactic (JSON well-formedness probe only) and
//! structural (blank ⇔ unconfigured).

use std::path::Path;

use crate::artifacts::{MCP_CONFIG_FILE, atomic_write, repo_artifact};
use crate::core::gitops;
use crate::error::AppError;

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
pub fn load_state(root: &Path) -> McpLoadState {
    let path = repo_artifact(root, MCP_CONFIG_FILE);
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

/// Execute a Save against `root`: classify, then either no-op, atomic-write
/// + checkpoint, or remove + checkpoint. Disk effects ALWAYS precede the
///   git effect, so a checkpoint failure never strands an un-written change
///   — and never swallows: the `AppError` propagates to the dialog.
pub fn apply_save(root: &Path, buffer: &str) -> Result<McpApplyReceipt, AppError> {
    // Writer section: config write + checkpoint shares the planning index.
    let _guard = crate::core::writer_gate::acquire();
    let st = load_state(root);
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
            let path = repo_artifact(root, MCP_CONFIG_FILE);
            atomic_write(&path, buffer).map_err(|e| AppError::Io {
                op: "write .koolade-packet/config/mcp.json".to_string(),
                detail: e.to_string(),
            })?;
            let sha = gitops::commit(root, UPDATE_SUBJECT, &[MCP_CONFIG_FILE.to_string()])?;
            Ok(McpApplyReceipt {
                op,
                short_sha: Some(short_sha(&sha)),
                malformed,
            })
        }
        McpSaveOp::Clear => {
            let path = repo_artifact(root, MCP_CONFIG_FILE);
            std::fs::remove_file(&path).map_err(|e| AppError::Io {
                op: "remove .koolade-packet/config/mcp.json".to_string(),
                detail: e.to_string(),
            })?;
            let sha = gitops::commit(root, CLEAR_SUBJECT, &[MCP_CONFIG_FILE.to_string()])?;
            Ok(McpApplyReceipt {
                op,
                short_sha: Some(short_sha(&sha)),
                malformed: None,
            })
        }
    }
}

/// First 7 chars of a git short SHA (git may yield fewer digits on young
/// histories; callers treat the result as an opaque label).
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

#[cfg(test)]
#[path = "mcp_io/tests.rs"]
mod tests;
