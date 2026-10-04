//! One planning turn, orchestrated off the UI thread (SPECIFICATION.md
//! §12–§13, §16–§20). Pipeline:
//!
//!   snapshot state ▶ context ▶ prompt ▶ harness (pi) ▶ extract envelope
//!   ▶ validate ▶ apply (+ownership synthesis) ▶ atomic writes ▶ git checkpoint
//!
//! Exactly one worker exists at a time (the UI refuses concurrent submits),
//! so the worker may freely mutate its private state clone; the UI adopts
//! the resulting `PlannerState` when `Applied` lands. Cancellation is
//! cooperative (flag polled between stream events by the harness driver).

use std::time::{Duration, Instant};

use crate::core::apply::ApplyReceipt;
use crate::core::state::PlannerState;
use crate::core::validation::NormalizedTurn;
use crate::error::AppError;
use crate::harness::LiveProgress;

mod controller;
mod execute;

pub use controller::TurnController;

/// Local inference can take hours; silence never shortens this deadline.
/// The user can still stop a running turn with Cancel.
pub const TURN_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);

/// Optional positive wall-clock budget in seconds, read when a turn begins.
/// Invalid, zero, or unrepresentable values fall back to the twelve-hour default.
pub fn configured_turn_timeout() -> Duration {
    std::env::var("KOOLADE_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .filter(|duration| Instant::now().checked_add(*duration).is_some())
        .unwrap_or(TURN_TIMEOUT)
}

/// Immutable inputs frozen when the turn starts; chat is snapshotted so late
/// UI typing cannot race the in-flight prompt.
#[derive(Clone)]
pub struct TurnInputs {
    pub state: PlannerState,
    pub user_message: String,
    /// (speaker, text) oldest → newest, in chat-log order.
    pub recent_chat: Vec<(String, String)>,
    pub purpose: crate::core::workflow::TurnPurpose,
    /// Stable target identity for a ComparePlans turn.
    pub comparison_feature: Option<String>,
}

pub enum TurnEvt {
    /// Live display snapshot from the harness; never authoritative state.
    Progress(LiveProgress),
    Done(Box<TurnOutcome>),
}

#[derive(Debug)]
pub enum TurnOutcome {
    /// Success: artifacts written; `commit_result` carries the checkpoint sha
    /// or the git error. Adoption proceeds either way — the files on disk
    /// already reflect the turn, and the state clone matches disk.
    Applied {
        state: Box<PlannerState>,
        receipt: ApplyReceipt,
        normalized: Box<NormalizedTurn>,
        commit_result: Result<String, AppError>,
        elapsed: Duration,
        stderr_tail: String,
    },
    /// Envelope rejected by validation: NOTHING written, no commit (§16).
    Rejected {
        problems: Vec<String>,
        final_text: String,
        elapsed: Duration,
    },
    HarnessFailed {
        error: AppError,
        elapsed: Duration,
    },
}

#[cfg(test)]
mod tests;
