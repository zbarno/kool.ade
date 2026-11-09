//! Local VM lifecycle authority, separate from Docker execution.

mod errors;
#[cfg(target_os = "linux")]
mod qemu;
#[cfg(not(target_os = "linux"))]
mod qemu_unsupported;
#[cfg(target_os = "linux")]
mod registry;
mod types;

pub use errors::ComputerBackendErrorV1;
#[cfg(target_os = "linux")]
pub use qemu::LocalKvmComputerBackend;
#[cfg(not(target_os = "linux"))]
pub use qemu_unsupported::LocalKvmComputerBackend;
pub use types::{
    ComputerBackend, ComputerBackendEventKindV1, ComputerBackendEventV1, ComputerBackendProbeV1,
    ComputerDisplayFrameV1, ComputerDisplayInputV1, ComputerGuestChannelV1,
    ComputerGuestReadinessV1, ComputerMachineHandleV1, ComputerMachineRecordV1,
    ComputerMachineStateV1, ComputerResourceLimitsV1,
};

/// Read-only check that the fixed QEMU launcher is a trusted native executable.
/// Runtime session preparation repeats this check immediately before launch.
pub(crate) fn verify_qemu_launcher_containment_v1() -> Result<(), ComputerBackendErrorV1> {
    #[cfg(target_os = "linux")]
    {
        qemu::verify_qemu_launcher_containment()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(ComputerBackendErrorV1::UnsupportedHost)
    }
}
