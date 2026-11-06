//! Local VM lifecycle authority, separate from Docker execution.

mod errors;
#[cfg(target_os = "linux")]
mod qemu;
#[cfg(not(target_os = "linux"))]
mod qemu_unsupported;
mod registry;
mod types;

pub use errors::ComputerBackendErrorV1;
#[cfg(target_os = "linux")]
pub use qemu::LocalKvmComputerBackend;
#[cfg(not(target_os = "linux"))]
pub use qemu_unsupported::LocalKvmComputerBackend;
pub use types::{
    ComputerBackend, ComputerBackendEventKindV1, ComputerBackendEventV1, ComputerBackendProbeV1,
    ComputerGuestChannelV1, ComputerGuestReadinessV1, ComputerMachineHandleV1,
    ComputerMachineRecordV1, ComputerMachineStateV1, ComputerResourceLimitsV1,
};
