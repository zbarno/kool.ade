mod observation;

pub use observation::{GuestObservationV1, GuestWindowObservationV1};

use crate::computer_use::{ComputerActionPayloadV1, ComputerActionV1};
use serde::{Deserialize, Serialize};

pub const GUEST_PROTOCOL_VERSION_V1: u16 = 1;
pub const GUEST_CHANNEL_CAPABILITY_V1: &str = "computer_use.channel.v1";
pub const GUEST_ARTIFACT_CAPABILITY_V1: &str = "computer_use.artifact.v1";
pub const GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuestSenderV1 {
    Host,
    Guest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuestEnvelopeContextV1 {
    pub sender: GuestSenderV1,
    pub sequence: u64,
    pub request_id: String,
    pub action_id: String,
    pub deadline_unix_ms: u64,
    pub capability_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestCapabilitySetV1 {
    pub screen_capture: bool,
    pub pointer_input: bool,
    pub keyboard_input: bool,
    pub accessibility_tree: bool,
    pub app_registry: Vec<String>,
    /// True only when the guest has the restricted signed-package helper.
    #[serde(default)]
    pub app_management: bool,
    /// Task artifacts require the authenticated staging protocol and confined launcher.
    /// Kept false until both are present in the guest image.
    #[serde(default)]
    pub staged_task_app_launch: bool,
}

impl GuestCapabilitySetV1 {
    pub(crate) fn validate(&self) -> Result<(), GuestProtocolErrorV1> {
        let unique_ids = self
            .app_registry
            .iter()
            .collect::<std::collections::HashSet<_>>();
        if self.app_registry.len() > 32
            || unique_ids.len() != self.app_registry.len()
            || self.app_registry.iter().any(|id| {
                id.is_empty()
                    || id.len() > 64
                    || !id.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                    })
            })
        {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        Ok(())
    }
}

/// Every message carries the same identity and fencing envelope. Guest claims
/// are validated against host-owned session and lease state before use.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestEnvelopeV1 {
    pub protocol_version: u16,
    pub sender: GuestSenderV1,
    pub session_id: String,
    pub worker_id: String,
    pub lease_generation: u64,
    pub sequence: u64,
    pub request_id: String,
    pub action_id: String,
    pub deadline_unix_ms: u64,
    pub capability_id: String,
    pub message: GuestMessageV1,
}

impl GuestEnvelopeV1 {
    pub(crate) fn validate_common(&self) -> Result<(), GuestProtocolErrorV1> {
        if self.protocol_version != GUEST_PROTOCOL_VERSION_V1
            || !canonical_uuid(&self.session_id)
            || !canonical_uuid(&self.worker_id)
            || self.lease_generation == 0
            || self.sequence == 0
            || !canonical_uuid(&self.request_id)
            || !canonical_uuid(&self.action_id)
            || self.capability_id.is_empty()
            || self.capability_id.len() > 96
            || !self
                .capability_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
        {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        if !matches!(&self.message, GuestMessageV1::ClientHello { .. })
            && self.deadline_unix_ms == 0
        {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GuestMessageV1 {
    ClientHello {
        boot_id: String,
        guest_nonce_hex: String,
        capabilities: GuestCapabilitySetV1,
    },
    HostChallenge {
        host_nonce_hex: String,
        transcript_sha256: String,
    },
    ClientProof {
        transcript_sha256: String,
    },
    HostReady {
        transcript_sha256: String,
        capabilities: GuestCapabilitySetV1,
    },
    ActionRequest {
        action: ComputerActionV1,
    },
    StageTaskApplicationStart {
        action: ComputerActionV1,
    },
    StageTaskArtifactChunk {
        artifact_index: u16,
        offset: u64,
        bytes_base64: String,
    },
    StageTaskApplicationFinish,
    StageTaskTransferReply {
        accepted: bool,
    },
    ActionReply {
        action_sequence: u64,
        result: GuestOperationResultV1,
    },
    Observation {
        observation: GuestObservationV1,
    },
    Heartbeat {
        boot_id: String,
        uptime_millis: u64,
    },
    GuestExit {
        reason: GuestExitReasonV1,
    },
    ArtifactWriteChunk {
        transfer_id: String,
        file_name: String,
        offset: u64,
        total_bytes: u64,
        sha256: String,
        bytes: Vec<u8>,
    },
    ArtifactWriteAck {
        transfer_id: String,
        next_offset: u64,
        complete: bool,
    },
    ArtifactAbort {
        transfer_id: String,
    },
    ArtifactReadChunk {
        transfer_id: String,
        file_name: String,
        offset: u64,
        max_bytes: u32,
    },
    ArtifactReadResult {
        transfer_id: String,
        offset: u64,
        total_bytes: u64,
        sha256: String,
        bytes: Vec<u8>,
        complete: bool,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuestExitReasonV1 {
    UnsupportedOperation,
    GuestInteractionFailed,
    InternalProtocolFailure,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum GuestOperationResultV1 {
    Completed,
    UnsupportedCapability,
    StaleObservation,
    FocusRequired,
    TimedOut,
    Rejected,
    Failed,
    MissingDependencies,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestActionReplyV1 {
    pub action_sequence: u64,
    pub result: GuestOperationResultV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestActionOutcomeV1 {
    pub reply: GuestActionReplyV1,
    pub observation: Option<GuestObservationV1>,
}

pub(crate) fn required_capability(action: &ComputerActionV1) -> &'static str {
    match &action.payload {
        ComputerActionPayloadV1::ObserveScreen | ComputerActionPayloadV1::ObserveWindows => {
            "computer_use.screen.v1"
        }
        ComputerActionPayloadV1::ReadAccessibility => "computer_use.accessibility.v1",
        ComputerActionPayloadV1::MouseClick { .. }
        | ComputerActionPayloadV1::MouseDoubleClick { .. }
        | ComputerActionPayloadV1::MouseMove { .. }
        | ComputerActionPayloadV1::MouseDrag { .. }
        | ComputerActionPayloadV1::MouseScroll { .. }
        | ComputerActionPayloadV1::FocusWindow { .. } => "computer_use.pointer.v1",
        ComputerActionPayloadV1::KeyPress { .. }
        | ComputerActionPayloadV1::KeyChord { .. }
        | ComputerActionPayloadV1::TypeText { .. } => "computer_use.keyboard.v1",
        ComputerActionPayloadV1::LaunchApp { .. } | ComputerActionPayloadV1::ManageApp { .. } => {
            "computer_use.app_registry.v1"
        }
        ComputerActionPayloadV1::LaunchApprovedStagedTaskApp { .. } => "computer_use.staged_app.v1",
        ComputerActionPayloadV1::WaitFor { .. } | ComputerActionPayloadV1::QueryState => {
            "computer_use.state.v1"
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestProtocolErrorV1 {
    Malformed,
    Oversized,
    AuthenticationFailed,
    WrongSession,
    StaleObservation,
    StaleLease,
    Replay,
    SequenceGap,
    DeadlineExpired,
    UnsupportedCapability,
    OutcomeUnknown,
    /// A structured guest failure arrived; the host journal leaves the action
    /// pending because the guest may have completed a side effect first.
    GuestExited(GuestExitReasonV1),
    Io,
}

impl std::fmt::Display for GuestProtocolErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "computer-use guest protocol error: {self:?}")
    }
}

impl std::error::Error for GuestProtocolErrorV1 {}

pub(crate) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
