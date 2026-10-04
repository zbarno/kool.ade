//! Git-backed time ledger (F7, Scope 3 / AD-3): durable interval rows in a
//! dedicated artifact slot under `.koolade-packet/`.
//!
//! Slot: `.koolade-packet/state/time-ledger.log`
//! ([`crate::artifacts::layout::canonical::LEDGER`]). The file carries one
//! header line ([`HEADER_LINE`]) followed by interval rows, one UTF-8 line
//! each, `:`-separated with a stable field order:
//!
//! ```text
//! T:<repo_id>:<workspace_id>:<session_id>:<worker_pid|->:<start_epoch_s>:<end_epoch_s|->:<item_uid>:<feature_ref|->:<end_status>
//! ```
//!
//! `<end_status>` is exactly one of `ended`, `interrupted-discard`,
//! `interrupted-count`; absence is the single-token `-`. Text fields are
//! non-empty printables with no `:`, `,`, or control characters; epochs and
//! PIDs are unsigned ASCII decimals. Line parsing and torn-tail salvage live
//! in the [`format`] submodule.
//!
//! # Count-vs-discard rule for interrupted intervals
//! Rows reach disk only through [`crate::artifacts::atomic_write_bytes`]
//! (temp + rename + dirsync), so a crash between write_all and rename leaves
//! either the complete pre-crash file or the complete post-crash file. The
//! torn-final-line handling in [`format`] is defense-in-depth for foreign or
//! pre-atomic writers and implements one consistent policy:
//!
//! * Any terminated row failing strict validation is corruption: [`load`] and
//!   [`append_row`] fail instead of guessing.
//! * The single unterminated final fragment (no closing newline, including
//!   stray-CR tails) is *salvaged*, never fatal: the longest cleanly decodable
//!   field prefix is kept, damaged or missing fields neutralize, and the
//!   status is forced to [`EndStatus::InterruptedDiscard`] — even when nine
//!   fields visibly decode, because the missing terminator proves the write
//!   stopped short. Billable sums MUST exclude `InterruptedDiscard` rows, so
//!   an interrupted interval is always counted-or-discarded the same way and
//!   can never be double-counted: at most one salvage row ever exists, and the
//!   next successful [`append_row`] crystallizes it into a stable, strictly
//!   valid discard row that reloads byte-identically.
//! * `EndStatus::InterruptedCount` reserves the opposite policy (credit a
//!   partial interval whose end instant is provable); v1 [`load`] never
//!   synthesizes it.
//!
//! # Coarse git commit triggers (wired in F7 task 6, not here)
//! Commit this ledger at coarse grain only — [`COARSE_COMMIT_TRIGGER_SET`] —
//! and reuse the FR-3 scoped checkpoint helper verbatim when wiring:
//! `crate::core::gitops::commit_planning_changes(cwd: &Path, message: &str,
//! paths: &[String]) -> Result<String, AppError>`, which commits only the
//! supplied paths under the shared repository lock.

use std::fs;
use std::path::{Path, PathBuf};

use crate::artifacts::atomic::atomic_write_bytes;
use crate::artifacts::layout;

/// Fixed first line of every ledger file; loading rejects any other lead line.
pub const HEADER_LINE: &str = "time-ledger:v1";

/// Candidate coarse commit grains for the git-backed ledger (see module docs).
pub const COARSE_COMMIT_TRIGGER_SET: &[&str] = &[
    "session-save-close",
    "day-rollover",
    "month-export",
    "explicit-flush",
];

/// Terminal disposition of one accrued agent-work interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndStatus {
    /// Normal stop: `end_epoch_s` present and not before the start.
    Ended,
    /// Interrupted and excluded from billable sums (torn rows land here).
    InterruptedDiscard,
    /// Interrupted but credited (reserved policy; v1 load never synthesizes).
    InterruptedCount,
}

impl EndStatus {
    fn token(self) -> &'static str {
        match self {
            Self::Ended => "ended",
            Self::InterruptedDiscard => "interrupted-discard",
            Self::InterruptedCount => "interrupted-count",
        }
    }

    pub(crate) fn from_token(token: &str) -> Option<Self> {
        match token {
            "ended" => Some(Self::Ended),
            "interrupted-discard" => Some(Self::InterruptedDiscard),
            "interrupted-count" => Some(Self::InterruptedCount),
            _ => None,
        }
    }
}

/// One durably persisted agent-work interval (all fields owned).
///
/// Timestamps are integer seconds on the device-local wall clock (AD-2:
/// anomalous clocks are corrected by the operator); sub-second resolution is
/// intentionally omitted to keep rows compact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntervalRow {
    pub repo_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub worker_pid: Option<u64>,
    pub start_epoch_s: u64,
    pub end_epoch_s: Option<u64>,
    pub item_uid: String,
    pub feature_ref: Option<String>,
    pub end_status: EndStatus,
}

/// Absolute ledger path inside a project repository root.
pub fn ledger_path(repo_root: &Path) -> PathBuf {
    repo_root.join(layout::canonical::LEDGER)
}

/// Serialize one row as its canonical v1 line, newline-terminated.
pub fn serialize_line(row: &IntervalRow) -> String {
    let optional = |value: Option<u64>| value.map_or_else(|| "-".to_owned(), |v| v.to_string());
    format!(
        "T:{}:{}:{}:{}:{}:{}:{}:{}:{}\n",
        row.repo_id,
        row.workspace_id,
        row.session_id,
        optional(row.worker_pid),
        row.start_epoch_s,
        optional(row.end_epoch_s),
        row.item_uid,
        row.feature_ref.clone().unwrap_or_else(|| "-".into()),
        row.end_status.token()
    )
}

/// Append one row: load the current rows (salvaging a torn final line), then
/// atomically rewrite the whole file as header + rows + the new row. A missing
/// ledger file starts a fresh one; interior corruption aborts without writing.
pub fn append_row(repo_root: &Path, row: &IntervalRow) -> anyhow::Result<()> {
    let rows = load(repo_root)?;
    let mut text = String::from(HEADER_LINE);
    text.push('\n');
    for stored in &rows {
        text.push_str(&serialize_line(stored));
    }
    text.push_str(&serialize_line(row));
    let path = ledger_path(repo_root);
    atomic_write_bytes(&path, text.as_bytes())?;
    Ok(())
}

/// Restore all intervals from `repo_root`. A missing file yields an empty vec
/// and creates nothing; loading never mutates the file.
pub fn load(repo_root: &Path) -> anyhow::Result<Vec<IntervalRow>> {
    let path = ledger_path(repo_root);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path)
        .map_err(|e| anyhow::anyhow!("cannot read time ledger {}: {e}", path.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("time ledger {} is not valid UTF-8", path.display()))?;
    // The trailing '\n'-free segment is the torn fragment when present.
    let (body, torn) = match text.rfind('\n') {
        Some(pos) => (&text[..pos], &text[pos + 1..]),
        None => ("", &text[..]),
    };
    let mut rows = parse_body(&path, body)?;
    if !torn.is_empty() {
        rows.push(format::salvage_torn_fragment(torn));
    }
    Ok(rows)
}

fn parse_body(path: &Path, body: &str) -> anyhow::Result<Vec<IntervalRow>> {
    let lines: Vec<&str> = body.lines().collect();
    if lines.first() != Some(&HEADER_LINE) {
        return Err(anyhow::anyhow!(
            "time ledger {} must open with header '{HEADER_LINE}'",
            path.display()
        ));
    }
    let mut rows = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(1) {
        let row = format::decode_line(line).map_err(|why| {
            anyhow::anyhow!(
                "time ledger {} corrupt at line {index}: {why}",
                path.display()
            )
        })?;
        rows.push(row);
    }
    Ok(rows)
}

mod format;

#[cfg(test)]
mod tests;
