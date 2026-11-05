use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ComputerOperationV1 {
    ObserveScreen,
    ReadAccessibility,
    MouseInput,
    KeyboardInput,
    LaunchAllowedApp,
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
    ReadAccessibility,
    MouseClick {
        x: u32,
        y: u32,
        button: MouseButtonV1,
    },
    KeyPress {
        key: KeyCodeV1,
    },
    TypeText {
        text: String,
    },
    LaunchApp {
        app_id: String,
    },
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
}

impl ComputerActionV1 {
    /// Bind an untrusted action request to the actor authenticated by the
    /// controller ingress. Never take the actor value as proof of identity.
    pub fn validate_as(
        &self,
        authenticated_actor: ComputerActionActorV1,
        session: &super::ComputerSessionV1,
    ) -> Result<(), &'static str> {
        if self.actor != authenticated_actor {
            return Err("action actor does not match authenticated caller");
        }
        self.validate(session)
    }

    pub fn validate(&self, session: &super::ComputerSessionV1) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported action version");
        }
        let policy = &session.policy;
        policy.validate()?;
        if !is_uuid(&self.session_id) || !is_uuid(&self.idempotency_key) {
            return Err("invalid action identity");
        }
        if self.session_id != session.identity.session_id
            || self.lease_generation != session.lease.generation
            || self.approval_generation != policy.approval_generation
            || self.cancellation_generation != session.cancellation_generation
            || self.action_sequence > policy.resource_budget.action_limit as u64
        {
            return Err("action scope or approval generation mismatch");
        }
        if self.action_sequence == 0
            || self.approval_generation == 0
            || self.cancellation_generation == 0
        {
            return Err("action generations and sequence must be positive");
        }
        if !policy.authorized_operations.contains(&self.operation) {
            return Err("action operation is not authorized");
        }
        let payload_matches = matches!(
            (&self.operation, &self.payload),
            (
                ComputerOperationV1::ObserveScreen,
                ComputerActionPayloadV1::ObserveScreen
            ) | (
                ComputerOperationV1::ReadAccessibility,
                ComputerActionPayloadV1::ReadAccessibility
            ) | (
                ComputerOperationV1::MouseInput,
                ComputerActionPayloadV1::MouseClick { .. }
            ) | (
                ComputerOperationV1::KeyboardInput,
                ComputerActionPayloadV1::KeyPress { .. }
            ) | (
                ComputerOperationV1::KeyboardInput,
                ComputerActionPayloadV1::TypeText { .. }
            ) | (
                ComputerOperationV1::LaunchAllowedApp,
                ComputerActionPayloadV1::LaunchApp { .. }
            )
        );
        if !payload_matches {
            return Err("action payload does not match its operation");
        }
        match &self.payload {
            ComputerActionPayloadV1::MouseClick { x, y, .. } if *x > 16_384 || *y > 16_384 => {
                return Err("pointer coordinate exceeds display limit");
            }
            ComputerActionPayloadV1::TypeText { text }
                if text.len() > 4096 || text.contains('\0') =>
            {
                return Err("text input exceeds bounds");
            }
            ComputerActionPayloadV1::LaunchApp { app_id }
                if app_id.is_empty()
                    || app_id.len() > 64
                    || !app_id.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                    }) =>
            {
                return Err("app identifier is invalid");
            }
            ComputerActionPayloadV1::LaunchApp { app_id }
                if !policy.allowed_apps.contains(app_id) =>
            {
                return Err("app is not in the session allowlist");
            }
            _ => {}
        }
        Ok(())
    }
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
