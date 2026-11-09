//! Contracts for isolated graphical Computer Use sessions.
//!
//! These contracts intentionally do not share authority with Docker execution.
//! Typed VM lifecycle operations bind them to the local Linux QEMU/KVM backend.

pub mod apps;
pub mod artifacts;
mod backend;
mod contract;
pub mod guest_image;
#[cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, unused_imports, unused_variables)
)]
pub mod guest_protocol;
#[cfg(target_os = "linux")]
mod host_executable;
pub mod mcp;
#[cfg(target_os = "linux")]
pub(crate) mod network;
#[cfg(target_os = "linux")]
pub(crate) mod personal_storage;
#[cfg(target_os = "linux")]
pub use personal_storage::{
    PersonalVmProfileStoreV1, PersonalVmProfileV1, PersonalVmStorageErrorV1,
};
#[cfg(target_os = "linux")]
pub(crate) mod process_security;

#[cfg(target_os = "linux")]
pub(crate) use host_executable::validate_trusted_system_executable;

pub(crate) use backend::verify_qemu_launcher_containment_v1;
pub use backend::{
    ComputerBackend, ComputerBackendErrorV1, ComputerBackendEventKindV1, ComputerBackendEventV1,
    ComputerBackendProbeV1, ComputerDisplayFrameV1, ComputerDisplayInputV1, ComputerGuestChannelV1,
    ComputerGuestReadinessV1, ComputerMachineHandleV1, ComputerMachineRecordV1,
    ComputerMachineStateV1, ComputerResourceLimitsV1, LocalKvmComputerBackend,
};
pub use contract::*;
