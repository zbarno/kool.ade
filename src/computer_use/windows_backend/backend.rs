//! Typed Windows backend boundary. It remains deliberately unavailable until
//! app-owned QEMU trust, bounded overlay growth, authenticated transport,
//! durable recovery, and native display control are all evidenced.

use std::time::Duration;

use crate::computer_use::{
    ComputerBackendErrorV1 as Error, ComputerBackendProbeV1 as Probe, ComputerHostPlatformV1,
};

use super::WindowsWpxReadiness;
#[cfg(not(windows))]
use super::assess_windows_wpx;

mod lifecycle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowsBackendBlockerV1 {
    HostPrerequisitesUnavailable,
    TrustedQemuRuntimeMissing,
    OverlayGrowthQuotaMissing,
    GuestTransportEvidenceMissing,
    DurableRecoveryMissing,
    DisplayControlMissing,
    ReleaseGateClosed,
}

impl WindowsBackendBlockerV1 {
    fn diagnostic(self) -> &'static str {
        match self {
            Self::HostPrerequisitesUnavailable => {
                "WHPX prerequisites are unavailable or unverified."
            }
            Self::TrustedQemuRuntimeMissing => {
                "No app-owned, provenance-verified Windows QEMU runtime is configured."
            }
            Self::OverlayGrowthQuotaMissing => {
                "Windows VM overlay growth has no enforced physical storage quota."
            }
            Self::GuestTransportEvidenceMissing => {
                "Windows QEMU AF_UNIX client and guest peer evidence are incomplete."
            }
            Self::DurableRecoveryMissing => {
                "Windows machine ownership and crash recovery records are not implemented."
            }
            Self::DisplayControlMissing => {
                "Windows VM display capture and local input are not integrated."
            }
            Self::ReleaseGateClosed => {
                "Windows Computer Use remains disabled until its host release gate passes."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WindowsBackendEvidenceV1 {
    pub(crate) trusted_qemu_runtime: bool,
    pub(crate) overlay_growth_quota: bool,
    pub(crate) guest_transport: bool,
    pub(crate) durable_recovery: bool,
    pub(crate) display_control: bool,
    pub(crate) release_enabled: bool,
}

impl WindowsBackendEvidenceV1 {
    pub(crate) fn blockers(self) -> Vec<WindowsBackendBlockerV1> {
        let mut blockers = Vec::new();
        if !self.trusted_qemu_runtime {
            blockers.push(WindowsBackendBlockerV1::TrustedQemuRuntimeMissing);
        }
        if !self.overlay_growth_quota {
            blockers.push(WindowsBackendBlockerV1::OverlayGrowthQuotaMissing);
        }
        if !self.guest_transport {
            blockers.push(WindowsBackendBlockerV1::GuestTransportEvidenceMissing);
        }
        if !self.durable_recovery {
            blockers.push(WindowsBackendBlockerV1::DurableRecoveryMissing);
        }
        if !self.display_control {
            blockers.push(WindowsBackendBlockerV1::DisplayControlMissing);
        }
        if !self.release_enabled {
            blockers.push(WindowsBackendBlockerV1::ReleaseGateClosed);
        }
        blockers
    }
}

/// Reports precise missing Windows runtime evidence and implements the shared
/// backend contract without allowing startup until every boundary is verified.
pub(crate) struct WindowsComputerBackend {
    platform: ComputerHostPlatformV1,
    readiness: WindowsWpxReadiness,
    evidence: WindowsBackendEvidenceV1,
}

impl WindowsComputerBackend {
    #[allow(dead_code)] // Shared target selection remains behind the host release gate.
    pub(crate) fn new() -> Self {
        Self::with_platform_readiness(ComputerHostPlatformV1::current(), native_readiness())
    }

    fn with_platform_readiness(
        platform: ComputerHostPlatformV1,
        readiness: WindowsWpxReadiness,
    ) -> Self {
        Self {
            platform,
            readiness,
            // No app-owned Windows QEMU runtime or physical overlay quota is
            // installed, so host capabilities alone cannot pass this gate.
            evidence: WindowsBackendEvidenceV1::default(),
        }
    }

    pub(crate) fn blockers(&self) -> Vec<WindowsBackendBlockerV1> {
        let mut blockers = Vec::new();
        if !self.readiness.ready_for_adapter {
            blockers.push(WindowsBackendBlockerV1::HostPrerequisitesUnavailable);
        }
        blockers.extend(self.evidence.blockers());
        blockers
    }

    pub(crate) fn diagnostics(&self) -> Vec<&'static str> {
        self.blockers()
            .into_iter()
            .map(WindowsBackendBlockerV1::diagnostic)
            .collect()
    }

    fn unavailable_error(&self) -> Error {
        if !self.readiness.ready_for_adapter {
            return Error::HostNotReady;
        }
        match self.blockers().first() {
            Some(WindowsBackendBlockerV1::TrustedQemuRuntimeMissing) => Error::QemuUnavailable,
            Some(WindowsBackendBlockerV1::OverlayGrowthQuotaMissing) => Error::ResourceLimit,
            Some(WindowsBackendBlockerV1::GuestTransportEvidenceMissing) => {
                Error::GuestChannelUnavailable
            }
            _ => Error::UnsupportedHost,
        }
    }

    fn probe_for(&self) -> Probe {
        Probe {
            host_platform: Some(self.platform),
            hypervisor_accessible: self.readiness.ready_for_adapter,
            // These remain false until an app-owned runtime resolver and
            // physical disk-growth enforcement are integrated.
            qemu_available: self.evidence.trusted_qemu_runtime,
            delegated_resource_controls: self.evidence.overlay_growth_quota,
            max_vcpus: 0,
            max_memory_bytes: 0,
            max_overlay_bytes: 0,
            max_processes: 0,
            parallel_slots_per_machine: 0,
            boot_timeout: Duration::ZERO,
        }
    }
}

#[allow(dead_code)] // Used when the Windows adapter is selected by controller startup.
fn native_readiness() -> WindowsWpxReadiness {
    #[cfg(windows)]
    {
        super::probe_windows_wpx()
    }
    #[cfg(not(windows))]
    {
        assess_windows_wpx(ComputerHostPlatformV1::current(), None, None, None)
    }
}

#[cfg(test)]
#[path = "backend/tests.rs"]
mod tests;
