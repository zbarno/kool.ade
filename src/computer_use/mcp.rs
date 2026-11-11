//! Versioned, provider-neutral Computer Use MCP request and response contracts.

#[cfg(target_os = "linux")]
mod authorized_host;
#[cfg(target_os = "linux")]
pub use authorized_host::{ComputerUseMcpOperationsV1, RegistryBoundComputerUseMcpHostV1};
mod consent;
#[cfg(target_os = "linux")]
mod controller;
pub use consent::{
    ComputerUseConsentBrokerV1, ComputerUseConsentDecisionV1, ComputerUseConsentGrantV1,
    ComputerUseConsentRequestV1, ComputerUseConsentScopeV1,
};
#[cfg(target_os = "linux")]
pub use controller::{ComputerUseMcpControllerV1, ComputerUseMcpRuntimeConfigV1};

use serde::{Deserialize, Serialize};

use super::{
    ComputerActionActorV1, ComputerActionIntentStateV1, ComputerActionIntentV1,
    ComputerActionPayloadV1, ComputerIdentityV1,
    guest_protocol::{GuestActionOutcomeV1, GuestObservationV1},
};

pub const COMPUTER_USE_MCP_VERSION_V1: u16 = 1;
pub const MAX_COMPUTER_USE_PURPOSE_BYTES: usize = 512;
pub const MAX_COMPUTER_USE_SUMMARY_BYTES: usize = 512;
pub const MAX_COMPUTER_USE_ACTION_TIMEOUT_MS: u32 = 60_000;
pub const COMPUTER_USE_RUNTIME_QA_ROLE_V1: &str = "runtime_qa/v3";
pub const COMPUTER_USE_RUNTIME_QA_APP_ID_V1: &str = "qa-task-app";
pub const SUPPORTED_COMPUTER_USE_ROLES_V1: [&str; 3] = [
    "implementer/v1",
    "repair/v1",
    COMPUTER_USE_RUNTIME_QA_ROLE_V1,
];
pub const RUNTIME_QA_COMPUTER_USE_OPERATIONS_V1: [crate::computer_use::ComputerOperationV1; 7] = [
    crate::computer_use::ComputerOperationV1::ObserveScreen,
    crate::computer_use::ComputerOperationV1::ObserveWindows,
    crate::computer_use::ComputerOperationV1::MouseInput,
    crate::computer_use::ComputerOperationV1::KeyboardInput,
    crate::computer_use::ComputerOperationV1::LaunchApprovedStagedTaskApp,
    crate::computer_use::ComputerOperationV1::WaitFor,
    crate::computer_use::ComputerOperationV1::QueryState,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerUseControlOwnerV1 {
    Agent,
    Human,
    Paused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerUseTaskSessionStatusV1 {
    Queued,
    Starting,
    Active,
    Interrupted,
    Uncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComputerUseControlCommandV1 {
    PauseAgent,
    ResumeAgent,
    TakeControl,
    ReturnControl,
    StopComputer,
    InspectUncertainOutcome,
    LaunchApprovedApp(String),
    ManageApprovedApp {
        app_id: String,
        operation: crate::computer_use::apps::GuestAppOperationKindV1,
    },
    ApproveSensitiveAction {
        intent_id: String,
    },
    DenySensitiveAction {
        intent_id: String,
    },
    VerifySensitiveActionCompleted {
        intent_id: String,
    },
    MarkSensitiveActionOutcomeUnknown {
        intent_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUseActionSummaryV1 {
    pub actor: ComputerUseControlOwnerV1,
    pub label: String,
    pub occurred_at_unix_ms: u64,
}

/// Display-only local view of a pending exact-action approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUseActionIntentViewV1 {
    pub intent: ComputerActionIntentV1,
    pub final_action: ComputerActionPayloadV1,
    pub state: ComputerActionIntentStateV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUseTaskSessionViewV1 {
    pub session_id: String,
    pub task_id: String,
    pub user_id: String,
    pub agent_role: String,
    pub session_class: super::ComputerSessionClassV1,
    pub allowed_apps: Vec<String>,
    pub guest_apps: Vec<ComputerUseGuestAppViewV1>,
    pub guest_app_catalog_status: String,
    pub app_management_available: bool,
    pub resource_budget: super::ComputerResourceBudgetV1,
    pub actions_remaining: u32,
    pub lifecycle: super::ComputerLifecycleV1,
    pub status: ComputerUseTaskSessionStatusV1,
    pub queue_position: Option<usize>,
    pub control_owner: ComputerUseControlOwnerV1,
    pub control_generation: u64,
    pub active_app: Option<String>,
    pub remaining_ms: u64,
    pub expires_at_unix_ms: u64,
    pub frame_revision: u64,
    pub frame_width: u16,
    pub frame_height: u16,
    pub frame_png: Option<Vec<u8>>,
    pub frame_error: bool,
    pub private_input: bool,
    pub control_transition_pending: bool,
    pub control_error: Option<String>,
    pub control_notice: Option<String>,
    pub action_log: Vec<ComputerUseActionSummaryV1>,
    pub pending_action_intent: Option<ComputerUseActionIntentViewV1>,
}

/// Display-only local inventory entry for an encrypted personal VM profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUsePersonalProfileViewV1 {
    pub profile_id: String,
    pub created_at_unix_ms: u64,
    pub retention_until_unix_ms: u64,
    pub active_session_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComputerUsePersonalVmChoiceV1 {
    Create { retention_days: u16 },
    Unlock { profile_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUsePersonalVmSessionTargetV1 {
    pub identity: super::ComputerIdentityV1,
    pub profile_id: String,
    pub session_class: super::ComputerSessionClassV1,
    pub network_profile: super::ComputerNetworkProfileV1,
    pub initial_control_owner: ComputerUseControlOwnerV1,
    pub private_input_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUsePersonalVmConsentViewV1 {
    pub request_id: String,
    pub choice: ComputerUsePersonalVmChoiceV1,
    pub target: ComputerUsePersonalVmSessionTargetV1,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerUseGuestAppViewV1 {
    pub app_id: String,
    pub display_name: String,
    pub publisher: String,
    pub source_id: String,
    pub provenance: crate::computer_use::apps::GuestAppProvenanceV1,
    pub version: String,
    pub package_id: String,
    pub package_sha256: String,
    pub manifest_sha256: String,
    pub required_capabilities: Vec<crate::computer_use::apps::GuestAppCapabilityV1>,
    pub installed: bool,
    pub session_allowed: bool,
}

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
        /// Controller-generated operation identity. It is honored only for a
        /// human-only ManageApp action; it never conveys authority.
        #[serde(default)]
        idempotency_key: Option<String>,
    },
    ProposeSensitiveAction {
        session_id: String,
        intent: Box<ComputerActionIntentV1>,
        final_action: Box<ComputerActionPayloadV1>,
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
        ComputerUseMcpRequestV1::Action {
            payload,
            idempotency_key: Some(key),
            ..
        } if !matches!(payload, ComputerActionPayloadV1::ManageApp { .. })
            || !canonical_uuid(key) =>
        {
            Err("Computer Use operation identity is invalid")
        }
        ComputerUseMcpRequestV1::Action {
            payload: ComputerActionPayloadV1::ManageApp { .. },
            idempotency_key: None,
            ..
        } => Err("managed app actions require a controller operation identity"),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "mcp/tests.rs"]
mod tests;

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}
