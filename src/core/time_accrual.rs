//! Per-workspace time accrual (F7, Scope 2 / AD-4, CLR-029): turns agent
//! execution spans into ledger intervals.
//!
//! Encoded rules (AC1, AC3, AC6):
//! * **Only active execution accrues** — a workspace slot is open exactly
//!   while the worker's agent process is alive. Every stoppage closes the
//!   interval: operator-wait (unanswered question or held approval), idle,
//!   paused, dormant, cancellation, or app close. Resuming a run opens a
//!   FRESH interval dated at the resume instant (backfill-zero).
//! * **Union on overlap, per workspace** — at most one open slot per
//!   workspace: a second overlapping start is suppressed, so a workspace
//!   never accrues faster than one-times wall rate.
//! * **Noise floor** — closed spans under [`MIN_ACCRUED_SECONDS`] are
//!   dropped without a ledger row, so sub-second start-stop churn is quiet.
//! * **App close** — [`app_close_flush`] finalizes every open slot as
//!   [`EndStatus::InterruptedDiscard`] (excluded from billable sums) so a
//!   restarted process can neither recreate nor double-count the interval.
//!
//! The engine ([`Accruer`]) is pure and unit-tested in isolation; the
//! process-global host ([`host`]) appends closed intervals to the git-backed
//! ledger of F7 task 1 ([`crate::artifacts::time_ledger::append_row`]).
//! Rows carry the project's root repository id as `repo_id` and the task's
//! target repository id as `workspace_id`; `feature_ref` stays `None`
//! until F7 task 3 enriches it read-time. Metering is best-effort: a
//! resolution or write failure surfaces an activity note, never disturbs
//! the work itself.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::artifacts::time_ledger::{EndStatus, IntervalRow};

use std::time::{SystemTime, UNIX_EPOCH};

mod host;

pub use host::{
    SpanGuard, activate_project, app_close_flush, drain_errors, root_repo_id, span_begin,
    span_for_ticket,
};

/// Smallest closed-span duration that earns a ledger row.
pub const MIN_ACCRUED_SECONDS: u64 = 1;

/// Device wall clock in integer seconds (AD-2: operators correct anomalies).
pub fn unix_now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Why an open slot was finalized. Every reason emits an [`EndStatus::Ended`]
/// row; discarding is reserved for app close. Distinctions stay for the F7
/// task 3 report consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The agent process finished (completion or corrective attempt).
    Completed,
    /// Operator-wait: an unanswered question or a held approval.
    OperatorWait,
    /// The worker went idle (nothing queued for it).
    Idle,
    /// The operator paused or cancelled the run.
    Paused,
    /// The app went dormant (reserved for power-management hooks).
    Dormant,
    /// The application closed while the agent was still running.
    AppClose,
}

/// Outcome of trying to open a workspace slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartOutcome {
    /// A fresh slot was created and accrual started.
    Opened,
    /// The workspace already had an open slot; overlapped time counts once.
    OverlapSuppressed,
}

/// Attribution carried by every open slot and emitted interval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSpan {
    /// Project root holding the ledger artifact.
    pub repo_root: PathBuf,
    /// Stable id of the project root repository (row `repo_id`).
    pub repo_id: String,
    /// Stable id of the task's target repository (row `workspace_id`).
    pub workspace_id: String,
    /// Stable task uid, or the ticket stem as fallback (row `item_uid`).
    pub item_uid: String,
}

struct OpenSlot {
    started_at: u64,
    session_id: String,
    span: AgentSpan,
}

/// A closed interval, ready for the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedInterval {
    pub span: AgentSpan,
    pub session_id: String,
    pub start_epoch_s: u64,
    pub end_epoch_s: u64,
    pub reason: StopReason,
    pub end_status: EndStatus,
}

impl ClosedInterval {
    pub fn duration_secs(&self) -> u64 {
        self.end_epoch_s.saturating_sub(self.start_epoch_s)
    }

    /// The ledger row for this interval. `worker_pid` is unknown to the
    /// meter; `feature_ref` is enriched read-time by F7 task 3.
    pub fn to_row(&self) -> IntervalRow {
        IntervalRow {
            repo_id: self.span.repo_id.clone(),
            workspace_id: self.span.workspace_id.clone(),
            session_id: self.session_id.clone(),
            worker_pid: None,
            start_epoch_s: self.start_epoch_s,
            end_epoch_s: Some(self.end_epoch_s),
            item_uid: self.span.item_uid.clone(),
            feature_ref: None,
            end_status: self.end_status,
        }
    }
}

/// Pure per-workspace accrual engine. No file access; rows are returned to
/// the caller, which decides where they go.
#[derive(Default)]
pub struct Accruer {
    open: BTreeMap<String, OpenSlot>,
    emitted: u64,
    suppressed_starts: u64,
    dropped_subsecond: u64,
}

impl Accruer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self, workspace_id: &str) -> bool {
        self.open.contains_key(workspace_id)
    }

    pub fn open_count(&self) -> usize {
        self.open.len()
    }

    pub fn open_workspace_ids(&self) -> Vec<&str> {
        self.open.keys().map(String::as_str).collect()
    }

    /// `(emitted, suppressed_starts, dropped_subsecond)` counters.
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.emitted, self.suppressed_starts, self.dropped_subsecond)
    }

    /// Opens the `span.workspace_id` slot at `now` (integer epoch seconds).
    /// A slot already open for that workspace suppresses the start
    /// (union-on-overlap).
    pub fn on_active_start(
        &mut self,
        span: &AgentSpan,
        session_id: &str,
        now: u64,
    ) -> StartOutcome {
        if self.open.contains_key(&span.workspace_id) {
            self.suppressed_starts += 1;
            return StartOutcome::OverlapSuppressed;
        }
        self.open.insert(
            span.workspace_id.clone(),
            OpenSlot {
                started_at: now,
                session_id: session_id.to_owned(),
                span: span.clone(),
            },
        );
        StartOutcome::Opened
    }

    /// Closes the workspace slot, if any, applying the noise floor and the
    /// given disposition. Returns the row-ready interval when emitted.
    fn finalize(
        &mut self,
        workspace_id: &str,
        reason: StopReason,
        status: EndStatus,
        now: u64,
    ) -> Option<ClosedInterval> {
        let slot = self.open.remove(workspace_id)?;
        let duration = now.saturating_sub(slot.started_at);
        if duration < MIN_ACCRUED_SECONDS {
            self.dropped_subsecond += 1;
            return None;
        }
        self.emitted += 1;
        Some(ClosedInterval {
            span: slot.span,
            session_id: slot.session_id,
            start_epoch_s: slot.started_at,
            end_epoch_s: now,
            reason,
            end_status: status,
        })
    }

    /// Stops accrual for `workspace_id` (operator-wait, idle, paused,
    /// dormant, or completion). Backfill-zero: releasing an operator-wait
    /// does NOT credit the gap; the resumed run opens a fresh slot.
    pub fn on_active_stop(
        &mut self,
        workspace_id: &str,
        reason: StopReason,
        now: u64,
    ) -> Option<ClosedInterval> {
        self.finalize(workspace_id, reason, EndStatus::Ended, now)
    }

    /// Application shutdown: every open slot becomes
    /// [`EndStatus::InterruptedDiscard`] (billable sums exclude it), in
    /// ascending workspace order.
    pub fn on_app_close(&mut self, now: u64) -> Vec<ClosedInterval> {
        let ids: Vec<String> = self.open.keys().cloned().collect();
        ids.into_iter()
            .filter_map(|id| {
                self.finalize(
                    &id,
                    StopReason::AppClose,
                    EndStatus::InterruptedDiscard,
                    now,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
