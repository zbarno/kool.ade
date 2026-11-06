use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerLifecycleV1 {
    Preparing,
    Booting,
    Connecting,
    Ready,
    Delegated,
    HumanControl,
    Suspended,
    Interrupted,
    Failed,
    Recovering,
    Destroyed,
}

/// Explicit allowlist prevents a guest from inventing lifecycle transitions.
pub fn can_transition(from: ComputerLifecycleV1, to: ComputerLifecycleV1) -> bool {
    use ComputerLifecycleV1 as S;
    matches!(
        (from, to),
        (
            S::Preparing,
            S::Booting | S::Failed | S::Interrupted | S::Destroyed
        ) | (
            S::Booting,
            S::Connecting | S::Failed | S::Interrupted | S::Destroyed
        ) | (
            S::Connecting,
            S::Ready | S::Failed | S::Interrupted | S::Destroyed
        ) | (
            S::Ready,
            S::Delegated
                | S::HumanControl
                | S::Suspended
                | S::Interrupted
                | S::Failed
                | S::Destroyed
        ) | (
            S::Delegated,
            S::Ready | S::HumanControl | S::Suspended | S::Interrupted | S::Failed | S::Destroyed
        ) | (
            S::HumanControl,
            S::Ready | S::Suspended | S::Interrupted | S::Failed | S::Destroyed
        ) | (
            S::Suspended,
            S::Ready | S::Interrupted | S::Failed | S::Destroyed
        ) | (S::Interrupted, S::Recovering | S::Destroyed)
            | (S::Failed, S::Recovering | S::Destroyed)
            | (
                S::Recovering,
                S::Booting | S::Connecting | S::Ready | S::Interrupted | S::Failed | S::Destroyed
            )
    )
}
