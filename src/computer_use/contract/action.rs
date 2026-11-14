mod gui;
mod validation;

pub use gui::{
    ComputerFrameReferenceV1, ComputerKeyV1, ComputerWaitConditionV1, ComputerWindowBoundsV1,
};

use crate::computer_use::apps::GuestAppOperationKindV1;
use serde::{Deserialize, Serialize};
use sha2::Digest;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerActionHelperAuthorizationV1 {
    pub signer_key_id: String,
    pub phase: ComputerActionHelperPhaseV1,
    /// Random one-use capability identifier, also covered by the signature.
    pub nonce: String,
    pub signature_hex: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerActionHelperPhaseV1 {
    Stage,
    Launch,
    Manage,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ComputerOperationV1 {
    ObserveScreen,
    ObserveWindows,
    ReadAccessibility,
    MouseInput,
    KeyboardInput,
    LaunchAllowedApp,
    LaunchApprovedStagedTaskApp,
    ManageApp,
    WaitFor,
    QueryState,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerActionActorV1 {
    Agent,
    Human,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerActionPayloadV1 {
    ObserveScreen,
    ObserveWindows,
    ReadAccessibility,
    MouseClick {
        target: ComputerFrameReferenceV1,
        x: u32,
        y: u32,
        button: MouseButtonV1,
    },
    MouseDoubleClick {
        target: ComputerFrameReferenceV1,
        x: u32,
        y: u32,
        button: MouseButtonV1,
    },
    MouseMove {
        target: ComputerFrameReferenceV1,
        x: u32,
        y: u32,
    },
    MouseDrag {
        target: ComputerFrameReferenceV1,
        from_x: u32,
        from_y: u32,
        to_x: u32,
        to_y: u32,
        button: MouseButtonV1,
    },
    MouseScroll {
        target: ComputerFrameReferenceV1,
        x: u32,
        y: u32,
        delta_x: i16,
        delta_y: i16,
    },
    FocusWindow {
        window_id: String,
        observation_revision: u64,
    },
    KeyPress {
        key: ComputerKeyV1,
        window_id: String,
        observation_revision: u64,
    },
    KeyChord {
        keys: Vec<ComputerKeyV1>,
        window_id: String,
        observation_revision: u64,
    },
    TypeText {
        text: String,
        window_id: String,
        observation_revision: u64,
    },
    LaunchApp {
        app_id: String,
    },
    ManageApp {
        app_id: String,
        operation: GuestAppOperationKindV1,
        catalog_digest: String,
        image_digest: String,
        manifest_sha256: String,
        package_sha256: String,
    },
    LaunchApprovedStagedTaskApp {
        application: Box<crate::computer_use::apps::StagedTaskApplicationV1>,
    },
    WaitFor {
        condition: ComputerWaitConditionV1,
        timeout_ms: u32,
    },
    QueryState,
}

impl ComputerActionPayloadV1 {
    /// Hash the exact typed GUI operation for a one-time action approval.
    pub fn sha256(&self) -> Result<String, &'static str> {
        let bytes = serde_json::to_vec(self).map_err(|_| "computer action could not be encoded")?;
        Ok(format!("sha256:{:x}", sha2::Sha256::digest(bytes)))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButtonV1 {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyCodeV1 {
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    Control,
    Shift,
    Alt,
    Super,
}

/// Sequence and idempotency are scoped to one lease and one approved task.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerActionV1 {
    pub schema_version: u16,
    pub session_id: String,
    pub action_sequence: u64,
    pub idempotency_key: String,
    /// Set by the authenticated controller ingress, never by guest output.
    pub actor: ComputerActionActorV1,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub operation: ComputerOperationV1,
    pub payload: ComputerActionPayloadV1,
    /// Controller signature used by the privileged guest helper to reject
    /// actions forged by the unprivileged guest agent process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helper_authorization: Option<ComputerActionHelperAuthorizationV1>,
}

impl ComputerActionV1 {
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn sign_for_guest_helper(
        &mut self,
        signer_key_id: &str,
        phase: ComputerActionHelperPhaseV1,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Result<(), &'static str> {
        use ed25519_dalek::Signer as _;
        if signer_key_id.is_empty() || signer_key_id.len() > 128 {
            return Err("invalid helper signer key identity");
        }
        self.helper_authorization = None;
        let nonce = uuid::Uuid::new_v4().to_string();
        let payload = helper_authorization_payload(self, signer_key_id, phase, &nonce)?;
        let signature = signing_key.sign(&payload);
        self.helper_authorization = Some(ComputerActionHelperAuthorizationV1 {
            signer_key_id: signer_key_id.to_owned(),
            phase,
            nonce,
            signature_hex: signature
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        });
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows", test))]
    pub fn verify_guest_helper_authorization(
        &self,
        expected_key_id: &str,
        expected_phase: ComputerActionHelperPhaseV1,
        verifying_key: &ed25519_dalek::VerifyingKey,
    ) -> bool {
        use ed25519_dalek::Verifier as _;
        let Some(authorization) = &self.helper_authorization else {
            return false;
        };
        if authorization.signer_key_id != expected_key_id
            || authorization.phase != expected_phase
            || !uuid::Uuid::parse_str(&authorization.nonce)
                .is_ok_and(|id| id.to_string() == authorization.nonce)
            || authorization.signature_hex.len() != 128
            || !authorization
                .signature_hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return false;
        }
        let Ok(signature_bytes) = decode_helper_hex(&authorization.signature_hex) else {
            return false;
        };
        let Ok(signature) = ed25519_dalek::Signature::from_slice(&signature_bytes) else {
            return false;
        };
        helper_authorization_payload(self, expected_key_id, expected_phase, &authorization.nonce)
            .is_ok_and(|payload| verifying_key.verify(&payload, &signature).is_ok())
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows", test))]
    pub(crate) fn same_helper_action(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.helper_authorization = None;
        right.helper_authorization = None;
        left == right
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows", test))]
fn helper_authorization_payload(
    action: &ComputerActionV1,
    signer_key_id: &str,
    phase: ComputerActionHelperPhaseV1,
    nonce: &str,
) -> Result<Vec<u8>, &'static str> {
    let mut unsigned = action.clone();
    unsigned.helper_authorization = None;
    let encoded = serde_json::to_vec(&unsigned).map_err(|_| "action cannot be encoded")?;
    let mut payload = b"koolade.computer-action-guest-helper-authorization.v1\0".to_vec();
    payload.extend_from_slice(&(signer_key_id.len() as u32).to_be_bytes());
    payload.extend_from_slice(signer_key_id.as_bytes());
    payload.push(match phase {
        ComputerActionHelperPhaseV1::Stage => 1,
        ComputerActionHelperPhaseV1::Launch => 2,
        ComputerActionHelperPhaseV1::Manage => 3,
    });
    payload.extend_from_slice(&(nonce.len() as u32).to_be_bytes());
    payload.extend_from_slice(nonce.as_bytes());
    payload.extend_from_slice(&(encoded.len() as u64).to_be_bytes());
    payload.extend_from_slice(&encoded);
    Ok(payload)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows", test))]
fn decode_helper_hex(value: &str) -> Result<Vec<u8>, ()> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(());
    }
    pairs
        .iter()
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            Ok(digit(pair[0]).ok_or(())? * 16 + digit(pair[1]).ok_or(())?)
        })
        .collect()
}
