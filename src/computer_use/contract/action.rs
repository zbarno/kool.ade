mod gui;
mod validation;

pub use gui::{
    ComputerFrameReferenceV1, ComputerKeyV1, ComputerWaitConditionV1, ComputerWindowBoundsV1,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ComputerOperationV1 {
    ObserveScreen,
    ObserveWindows,
    ReadAccessibility,
    MouseInput,
    KeyboardInput,
    LaunchAllowedApp,
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
    WaitFor {
        condition: ComputerWaitConditionV1,
        timeout_ms: u32,
    },
    QueryState,
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
