//! Persisted coordination state for the "Prepare this computer" setup flow.
//!
//! Records where a deterministic coordination loop stands between restarts.
//! It never authorizes mutations: every executed operation still carries its
//! own scoped approval binding and attempt record.

use serde::{Deserialize, Serialize};

pub const SETUP_FLOW_SCHEMA_VERSION: u8 = 1;
pub const MAX_FLOW_ID_LEN: usize = 64;
pub const MAX_FLOW_TASK_REF_LEN: usize = 180;
pub const MAX_COMPLETED_RECORDS: usize = 16;
pub const MAX_SEEN_VERIFIED: usize = 64;
pub const MAX_SKIPPED_RECORDS: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowPhaseV1 {
    /// Started by the operator; the driver selects the next action.
    Running,
    /// One setup operation awaits its explicit consent decision.
    WaitingForConsent { task_id: String },
    /// The flow parks for a genuinely human step or unresolved failure.
    WaitingForUser {
        task_id: String,
        reason: UserActionReasonV1,
    },
    /// All active checks settled; an authoritative recheck gates completion.
    Finishing,
    /// User paused; resumes only on explicit request after a fresh read.
    Paused,
    /// Terminated successfully.
    Complete(DoneOutcomeV1),
    /// Terminated unsuccessfully or interrupted; resumable by the operator.
    Halted {
        reason: HaltReasonV1,
        detail_task: Option<String>,
    },
}

impl FlowPhaseV1 {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete(_) | Self::Halted { .. })
    }

    pub fn task_reference(&self) -> Option<&str> {
        match self {
            Self::WaitingForConsent { task_id } | Self::WaitingForUser { task_id, .. } => {
                Some(task_id.as_str())
            }
            Self::Halted { detail_task, .. } => detail_task.as_deref(),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserActionReasonV1 {
    /// Vendor login, MFA, or license step the operator performs personally.
    VendorAuthentication,
    /// An OS dialog, elevated prompt, virtualization toggle, or reboot.
    OperatingSystemPrompt,
    /// Choosing a product, model, or security option for themselves.
    ProductChoice,
    /// Follow written steps; Kool.ad/e cannot perform them safely.
    ManualInstructions,
    /// The operator refused the offered operation.
    DeclinedByOperator,
    /// Automatic retries exhausted for the operation.
    RepeatedFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaltReasonV1 {
    /// App restarted while the flow was active; nothing replays automatically.
    InterruptedByRestart,
    /// Previously verified readiness regressed after an authoritative scan.
    ReadinessChanged,
    /// Operator pressed stop; the flow can be resumed deliberately.
    StoppedByOperator,
    /// Multiple supported operations kept failing beyond their retry cap.
    RepeatedFailures,
    /// The coordination loop hit an invariant violation and failed closed.
    InternalError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoneOutcomeV1 {
    /// Every active check is verified.
    FullyPrepared,
    /// All verifiable checks passed; some checks cannot run on this OS.
    PreparedWithUnavailable { unavailable_count: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupFlowStateV1 {
    pub schema: u8,
    pub flow_id: String,
    pub phase: FlowPhaseV1,
    pub started_at: u64,
    pub updated_at: u64,
    /// Most recently verified setup task IDs (bounded, newest appended).
    pub last_completed: Vec<String>,
    /// Task IDs ever observed verified this flow; grounds readiness-change
    /// detection so a regressed check is surfaced, never repaired silently.
    pub seen_verified: Vec<String>,
    /// Operations the operator declined or told the flow to skip; excluded
    /// from the automated loop until an explicit restart of the flow.
    #[serde(default)]
    pub skipped: Vec<String>,
    /// Newest committed scan generation the flow has acted upon.
    pub last_scan_generation: u64,
}

impl SetupFlowStateV1 {
    pub fn start_new(flow_id: String, at_unix_ms: u64) -> anyhow::Result<Self> {
        let state = Self {
            schema: SETUP_FLOW_SCHEMA_VERSION,
            flow_id,
            phase: FlowPhaseV1::Running,
            started_at: at_unix_ms,
            updated_at: at_unix_ms,
            last_completed: Vec::new(),
            seen_verified: Vec::new(),
            skipped: Vec::new(),
            last_scan_generation: 0,
        };
        state.validate_identity()?;
        Ok(state)
    }

    pub fn advance(&mut self, at_unix_ms: u64, mutate: impl FnOnce(&mut Self) -> bool) {
        self.updated_at = at_unix_ms;
        if mutate(self) {
            self.trim_records();
        }
    }

    pub(crate) fn trim_records(&mut self) {
        self.last_completed.dedup();
        if self.last_completed.len() > MAX_COMPLETED_RECORDS {
            let drop = self.last_completed.len() - MAX_COMPLETED_RECORDS;
            self.last_completed.drain(..drop);
        }
        self.seen_verified.sort();
        self.seen_verified.dedup();
        if self.seen_verified.len() > MAX_SEEN_VERIFIED {
            let drop = self.seen_verified.len() - MAX_SEEN_VERIFIED;
            self.seen_verified.drain(..drop);
        }
        self.skipped.dedup();
        if self.skipped.len() > MAX_SKIPPED_RECORDS {
            self.skipped.truncate(MAX_SKIPPED_RECORDS);
        }
    }

    pub(crate) fn validate_identity(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.flow_id.len() >= 8 && self.flow_id.len() <= MAX_FLOW_ID_LEN,
            "malformed setup flow identity"
        );
        anyhow::ensure!(
            self.flow_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "malformed setup flow identity"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> SetupFlowStateV1 {
        SetupFlowStateV1::start_new("flow-unit-01".to_string(), 100).unwrap()
    }

    #[test]
    fn start_new_requires_a_bounded_identity() {
        assert!(SetupFlowStateV1::start_new("abc".into(), 1).is_err());
        assert!(SetupFlowStateV1::start_new("has space".into(), 1).is_err());
        assert!(SetupFlowStateV1::start_new("x".repeat(65), 1).is_err());
        let state = SetupFlowStateV1::start_new("flow-unit-01".into(), 100).unwrap();
        assert!(matches!(state.phase, FlowPhaseV1::Running));
        assert_eq!(state.schema, SETUP_FLOW_SCHEMA_VERSION);
    }

    #[test]
    fn terminal_phases_report_terminal_and_phase_references_resolve() {
        let mut state = flow();
        assert!(!state.phase.is_terminal());
        state.phase = FlowPhaseV1::WaitingForConsent {
            task_id: "setup:a:b".into(),
        };
        assert_eq!(state.phase.task_reference(), Some("setup:a:b"));
        state.phase = FlowPhaseV1::Complete(DoneOutcomeV1::FullyPrepared);
        assert!(state.phase.is_terminal());
        state.phase = FlowPhaseV1::Halted {
            reason: HaltReasonV1::StoppedByOperator,
            detail_task: Some("setup:x:y".into()),
        };
        assert!(state.phase.is_terminal());
        assert_eq!(state.phase.task_reference(), Some("setup:x:y"));
    }

    #[test]
    fn record_lists_are_deduplicated_and_trimmed_on_advance() {
        let mut state = flow();
        state.advance(200, |state| {
            for i in 0..40u32 {
                state.last_completed.push(format!("setup:t:{i:02}"));
                state.skipped.push(format!("setup:t:{i:02}"));
                state.seen_verified.push(if i % 2 == 0 {
                    format!("dup-{i}")
                } else {
                    format!("seen-{i:02}")
                });
            }
            true
        });
        assert_eq!(state.last_completed.len(), MAX_COMPLETED_RECORDS);
        assert!(state.last_completed.windows(2).all(|w| w[0] != w[1]));
        assert_eq!(state.skipped.len(), MAX_SKIPPED_RECORDS);
        assert!(
            state.seen_verified.windows(2).all(|w| w[0] < w[1]),
            "sorted+deduped"
        );
    }

    #[test]
    fn no_change_updates_skip_record_management() {
        let mut state = flow();
        state.last_completed.push("setup:seed:a".into());
        state.advance(300, |_| false);
        assert_eq!(state.last_completed.len(), 1);
        assert_eq!(state.updated_at, 300);
    }
}
