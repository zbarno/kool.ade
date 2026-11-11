//! Canonical state-bound "Review and approve plan" action task.
//!
//! At most one durable review card is maintained for every active feature
//! attached to a TaskIntent. The card presents the exact immutable plan
//! snapshot the user may approve. Its status derives only from typed
//! lifecycle evidence (persisted readiness, user-confirmation receipts,
//! review verdicts, workflow approval); chat text and card counts never
//! drive it, so the board never infers approval or readiness on its own.

use crate::core::state::PlannerState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::storage::now_ms;
use super::{Work, WorkKind, WorkStatus};

mod details;
mod records;
mod reduce;
#[cfg(test)]
mod tests;
pub const PLAN_CONFIRMATION_SCHEMA: u16 = 1;

/// Why a plan is not approvable yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanNotReadyCause {
    /// No valid persisted readiness assessment exists for the current
    /// specification revision (or it lags newer clarification records).
    AssessmentNeeded,
    /// A persisted assessment exists but lists outstanding work: blocking
    /// questions, unassessed axes, or untraced requirements.
    RequirementsOutstanding,
    /// The feature document could not be parsed into a plan snapshot.
    MalformedDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanConfirmationV1 {
    pub schema_version: u16,
    pub feature_uid: String,
    /// SHA-256 of the immutable plan snapshot presented for approval.
    pub snapshot_content_hash: String,
    pub snapshot_spec_revision: String,
    /// True when the change is a published product document, false while it
    /// is still an editable draft.
    pub pin_is_published: bool,
    pub ready: bool,
    pub cause: Option<PlanNotReadyCause>,
    pub blockers: Vec<String>,
    /// Specification revision for which a readiness-evaluation turn was last
    /// requested; prevents hot re-prompt loops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_prompted_for: Option<String>,
    /// The readiness state the user was last told about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announced_ready: Option<bool>,
    pub evaluated_at_unix_ms: u64,
}

impl PlanConfirmationV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == PLAN_CONFIRMATION_SCHEMA,
            "unsupported plan confirmation schema version {}",
            self.schema_version
        );
        anyhow::ensure!(
            is_sha256(&self.snapshot_content_hash),
            "plan confirmation references an unparsed plan snapshot"
        );
        anyhow::ensure!(
            uuid::Uuid::parse_str(&self.feature_uid).is_ok(),
            "plan confirmation references an unknown feature identity"
        );
        if self.ready {
            anyhow::ensure!(
                self.cause.is_none() && self.blockers.is_empty(),
                "ready plan confirmation still names blockers"
            );
        } else {
            anyhow::ensure!(
                self.cause.is_some() && !self.blockers.is_empty(),
                "blocked plan confirmation must name why it is blocked"
            );
        }
        Ok(())
    }
}

/// Transition worth telling the user about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanConfirmationAnnouncement {
    pub work_uid: String,
    pub work_key: String,
    pub feature_id: String,
    pub feature_title: String,
    pub ready: bool,
    pub created_this_pass: bool,
    pub excerpt: String,
}

/// One readiness-evaluation turn the app layer should try to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessEvaluationRequest {
    pub work_key: String,
    pub feature_id: String,
    pub feature_title: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlanConfirmationReport {
    pub changed: bool,
    pub announcements: Vec<PlanConfirmationAnnouncement>,
    pub evaluations_wanted: Vec<ReadinessEvaluationRequest>,
    pub duplicates_removed: usize,
}

/// Recompute every canonical plan-confirmation card from authoritative
/// lifecycle evidence. Idempotent: a second pass with identical evidence is
/// a no-op. Keys in `excluded_keys` must not be mutated (they belong to a
/// concurrent write).
pub fn reconcile(
    state: &PlannerState,
    work: &mut Vec<Work>,
    excluded_keys: &BTreeSet<String>,
) -> anyhow::Result<PlanConfirmationReport> {
    let mut report = PlanConfirmationReport::default();
    for (feature_id, body) in state.active_features.iter() {
        reduce::reconcile_feature(state, feature_id, body, work, &mut report, excluded_keys)?;
    }
    reduce::manage_evaluation_prompts(work.as_mut_slice(), &mut report, excluded_keys);
    Ok(report)
}

fn is_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}
