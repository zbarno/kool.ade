use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerFailureCodeV1 {
    UnsupportedHost,
    UnsupportedArchitecture,
    CapabilityUnavailable,
    ImageUnverified,
    GrantDenied,
    GuestLost,
    LeaseExpired,
    ReplayDenied,
    ProtocolViolation,
    ResourceLimit,
    UncertainSideEffect,
    Cancelled,
    InternalFailure,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerStatusV1 {
    Accepted,
    Running,
    Succeeded,
    Denied,
    Failed,
    Interrupted,
    Uncertain,
}
