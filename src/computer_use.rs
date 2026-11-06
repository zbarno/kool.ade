//! Contracts for isolated graphical Computer Use sessions.
//!
//! These contracts intentionally do not share authority with Docker execution.
//! Typed VM lifecycle operations bind them to the local Linux QEMU/KVM backend.

mod backend;
mod contract;
pub mod guest_image;

pub use backend::{
    ComputerBackend, ComputerBackendErrorV1, ComputerBackendEventKindV1, ComputerBackendEventV1,
    ComputerBackendProbeV1, ComputerGuestChannelV1, ComputerGuestReadinessV1,
    ComputerMachineHandleV1, ComputerMachineRecordV1, ComputerMachineStateV1,
    ComputerResourceLimitsV1, LocalKvmComputerBackend,
};
pub use contract::*;
