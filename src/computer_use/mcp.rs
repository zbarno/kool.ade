//! Versioned, provider-neutral Computer Use MCP request and response contracts.

mod authorized_host;
pub use authorized_host::{ComputerUseMcpOperationsV1, RegistryBoundComputerUseMcpHostV1};
mod consent;
mod controller;
pub use consent::{
    ComputerUseConsentBrokerV1, ComputerUseConsentDecisionV1, ComputerUseConsentGrantV1,
    ComputerUseConsentRequestV1, ComputerUseConsentScopeV1,
};
pub use controller::{ComputerUseMcpControllerV1, ComputerUseMcpRuntimeConfigV1};

use serde::{Deserialize, Serialize};

use super::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerIdentityV1,
    guest_protocol::{GuestActionOutcomeV1, GuestObservationV1},
};

pub const COMPUTER_USE_MCP_VERSION_V1: u16 = 1;
pub const MAX_COMPUTER_USE_PURPOSE_BYTES: usize = 512;
pub const MAX_COMPUTER_USE_SUMMARY_BYTES: usize = 512;
pub const MAX_COMPUTER_USE_ACTION_TIMEOUT_MS: u32 = 60_000;
pub const SUPPORTED_COMPUTER_USE_ROLES_V1: [&str; 2] = ["implementer/v1", "repair/v1"];

pub fn is_supported_computer_use_role_v1(role_id: &str) -> bool {
    SUPPORTED_COMPUTER_USE_ROLES_V1.contains(&role_id)
}

/// App-bound identity. MCP arguments never supply these authority fields.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerUseCallerV1 {
    pub identity: ComputerIdentityV1,
    pub actor: ComputerActionActorV1,
    /// Versioned Kool.ad/e role, such as `implementation/v1` or `runtime_qa/v3`.
    pub role_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseObservationKindV1 {
    Screenshot,
    Windows,
    Accessibility,
    State,
}

/// Inputs for the fixed tool set. Session, task, actor, lease and action
/// generations are injected by the application host after validating caller
/// identity; they cannot be supplied as provider tool arguments.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerUseMcpRequestV1 {
    Capabilities,
    RequestSession {
        purpose: String,
    },
    StartSession {
        session_id: String,
    },
    Observe {
        session_id: String,
        observation: ComputerUseObservationKindV1,
    },
    Action {
        session_id: String,
        payload: ComputerActionPayloadV1,
        timeout_ms: u32,
    },
    Pause {
        session_id: String,
    },
    Resume {
        session_id: String,
    },
    Release {
        session_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseMcpStatusV1 {
    Ready,
    Completed,
    Blocked,
    Denied,
    Failed,
}

/// Bounded result. Image bytes are returned through the MCP image content
/// block; text fields remain short and suitable for task activity summaries.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerUseMcpResponseV1 {
    pub schema_version: u16,
    pub status: ComputerUseMcpStatusV1,
    pub summary: String,
    pub reason_code: Option<String>,
    pub session_id: Option<String>,
    pub capabilities: Vec<String>,
    pub observation: Option<GuestObservationV1>,
    pub action_outcome: Option<GuestActionOutcomeV1>,
}

impl ComputerUseMcpResponseV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != COMPUTER_USE_MCP_VERSION_V1
            || self.summary.len() > MAX_COMPUTER_USE_SUMMARY_BYTES
            || self.reason_code.as_ref().is_some_and(|reason| {
                reason.is_empty()
                    || reason.len() > 64
                    || !reason.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit()
                    })
            })
            || self.capabilities.len() > 32
            || self.capabilities.iter().any(|capability| {
                capability.is_empty()
                    || capability.len() > 64
                    || !capability.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || byte == b'.'
                            || byte == b'_'
                    })
            })
            || self
                .session_id
                .as_ref()
                .is_some_and(|id| !canonical_uuid(id))
        {
            return Err("Computer Use MCP response is malformed or exceeds its bounds");
        }
        if let Some(observation) = &self.observation {
            observation
                .validate_metadata()
                .map_err(|_| "Computer Use observation metadata is invalid")?;
            if observation.screenshot_png_base64.len()
                > super::guest_protocol::MAX_SCREENSHOT_PNG_BYTES * 4 / 3 + 8
            {
                return Err("Computer Use screenshot exceeds its size limit");
            }
        }
        Ok(())
    }

    pub fn needs_attention(&self) -> bool {
        self.status == ComputerUseMcpStatusV1::Blocked
    }
}

pub trait ComputerUseMcpHostV1: Send + Sync {
    fn handle(
        &self,
        caller: &ComputerUseCallerV1,
        request: ComputerUseMcpRequestV1,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> ComputerUseMcpResponseV1;
}

pub fn validate_request(request: &ComputerUseMcpRequestV1) -> Result<(), &'static str> {
    match request {
        ComputerUseMcpRequestV1::RequestSession { purpose }
            if purpose.trim().is_empty() || purpose.len() > MAX_COMPUTER_USE_PURPOSE_BYTES =>
        {
            Err("Computer Use session purpose is empty or exceeds its size limit")
        }
        ComputerUseMcpRequestV1::StartSession { session_id }
        | ComputerUseMcpRequestV1::Observe { session_id, .. }
        | ComputerUseMcpRequestV1::Action { session_id, .. }
        | ComputerUseMcpRequestV1::Pause { session_id }
        | ComputerUseMcpRequestV1::Resume { session_id }
        | ComputerUseMcpRequestV1::Release { session_id }
            if !canonical_uuid(session_id) =>
        {
            Err("Computer Use session identity is invalid")
        }
        ComputerUseMcpRequestV1::Action { timeout_ms, .. }
            if !(1..=MAX_COMPUTER_USE_ACTION_TIMEOUT_MS).contains(timeout_ms) =>
        {
            Err("Computer Use action timeout is outside the supported bound")
        }
        _ => Ok(()),
    }
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}
