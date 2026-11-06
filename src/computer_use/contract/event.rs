use super::{ComputerActionV1, ComputerFailureCodeV1, ComputerLifecycleV1};
use serde::{Deserialize, Serialize};

mod validator;
pub use validator::ComputerEventValidator;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerObservationV1 {
    pub schema_version: u16,
    pub session_id: String,
    pub action_sequence: u64,
    pub observed_at_unix_ms: u64,
    pub width_px: u32,
    pub height_px: u32,
    pub screenshot_sha256: String,
    pub accessibility_snapshot_ref: Option<String>,
    pub evidence_refs: Vec<String>,
}

impl ComputerObservationV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 || !is_uuid(&self.session_id) {
            return Err("invalid observation version or identity");
        }
        if self.observed_at_unix_ms == 0 {
            return Err("observation time must be positive");
        }
        if !(1..=16_384).contains(&self.width_px) || !(1..=16_384).contains(&self.height_px) {
            return Err("observation dimensions exceed bounds");
        }
        if !valid_digest(&self.screenshot_sha256) {
            return Err("screenshot digest is invalid");
        }
        if self
            .accessibility_snapshot_ref
            .as_ref()
            .is_some_and(|v| !is_uuid(v))
            || self.evidence_refs.len() > 128
            || self.evidence_refs.iter().any(|v| !is_uuid(v))
        {
            return Err("observation evidence references are invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerEventKindV1 {
    StateChanged {
        from: ComputerLifecycleV1,
        to: ComputerLifecycleV1,
    },
    ActionAccepted {
        action: ComputerActionV1,
    },
    ActionCompleted {
        action_sequence: u64,
        status: super::ComputerStatusV1,
        failure: Option<ComputerFailureCodeV1>,
    },
    Observation {
        observation: ComputerObservationV1,
    },
    LeaseRevoked {
        cancellation_generation: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerEventV1 {
    pub schema_version: u16,
    pub session_id: String,
    pub sequence: u64,
    pub occurred_at_unix_ms: u64,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub kind: ComputerEventKindV1,
}

impl ComputerEventV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 || !is_uuid(&self.session_id) {
            return Err("invalid event version or identity");
        }
        if self.sequence == 0
            || self.occurred_at_unix_ms == 0
            || self.lease_generation == 0
            || self.approval_generation == 0
            || self.cancellation_generation == 0
        {
            return Err("event sequence, time and generations must be positive");
        }
        if let ComputerEventKindV1::StateChanged { from, to } = &self.kind
            && !super::can_transition(*from, *to)
        {
            return Err("forbidden lifecycle transition");
        }
        if let ComputerEventKindV1::Observation { ref observation } = self.kind {
            observation.validate()?;
        }
        Ok(())
    }
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}
