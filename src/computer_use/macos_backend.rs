//! macOS Hypervisor.framework prerequisite assessment.
//!
//! This probe does not launch a VM or certify Computer Use support. The
//! lifecycle adapter, guest image and integrated host release gate are still
//! required before either Intel or Apple Silicon can be enabled.

use super::backend::ComputerHostPlatformV1;

#[cfg(unix)]
#[allow(dead_code)] // Consumed by the macOS lifecycle adapter once it is wired in.
mod process;
#[allow(dead_code)] // The launch plan remains disconnected while containment is incomplete.
mod qemu_plan;
pub(crate) mod session_plan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacOsHvfBlocker {
    UnsupportedArchitecture,
    HypervisorUnavailable,
    ProbeUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MacOsHvfReadiness {
    pub(crate) ready_for_adapter: bool,
    pub(crate) blocker: Option<MacOsHvfBlocker>,
    /// Hardware virtualization alone never opens platform support.
    pub(crate) release_enabled: bool,
}

impl MacOsHvfReadiness {
    pub(crate) fn diagnostic(self) -> &'static str {
        match self.blocker {
            Some(MacOsHvfBlocker::UnsupportedArchitecture) => {
                "macOS Computer Use is unavailable on this host architecture."
            }
            Some(MacOsHvfBlocker::HypervisorUnavailable) => {
                "macOS Hypervisor.framework is unavailable; Computer Use cannot start a VM."
            }
            Some(MacOsHvfBlocker::ProbeUnavailable) => {
                "macOS virtualization readiness could not be verified; Computer Use remains unavailable."
            }
            None => {
                "macOS virtualization is available, but the Computer Use adapter and release gate are incomplete."
            }
        }
    }
}

pub(crate) fn assess_macos_hvf(
    platform: ComputerHostPlatformV1,
    hypervisor_supported: Option<bool>,
) -> MacOsHvfReadiness {
    let blocker = match platform {
        ComputerHostPlatformV1::MacOsIntel | ComputerHostPlatformV1::MacOsAppleSilicon => {
            match hypervisor_supported {
                Some(true) => None,
                Some(false) => Some(MacOsHvfBlocker::HypervisorUnavailable),
                None => Some(MacOsHvfBlocker::ProbeUnavailable),
            }
        }
        _ => Some(MacOsHvfBlocker::UnsupportedArchitecture),
    };
    MacOsHvfReadiness {
        ready_for_adapter: blocker.is_none(),
        blocker,
        release_enabled: false,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn probe_macos_hvf() -> MacOsHvfReadiness {
    let platform = ComputerHostPlatformV1::current();
    if !matches!(
        platform,
        ComputerHostPlatformV1::MacOsIntel | ComputerHostPlatformV1::MacOsAppleSilicon
    ) {
        return assess_macos_hvf(platform, None);
    }

    let mut supported = 0_i32;
    let mut length = std::mem::size_of_val(&supported);
    // SAFETY: the output pointer and length describe a writable i32. The
    // sysctl only reads the host Hypervisor.framework availability flag.
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.hv_support".as_ptr(),
            (&mut supported as *mut i32).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || length != std::mem::size_of_val(&supported) {
        assess_macos_hvf(platform, None)
    } else {
        assess_macos_hvf(platform, Some(supported != 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hvf_readiness_distinguishes_supported_and_blocked_hosts_without_release_enablement() {
        for platform in [
            ComputerHostPlatformV1::MacOsIntel,
            ComputerHostPlatformV1::MacOsAppleSilicon,
        ] {
            let ready = assess_macos_hvf(platform, Some(true));
            assert!(ready.ready_for_adapter);
            assert_eq!(ready.blocker, None);
            assert!(!ready.release_enabled);
            assert!(ready.diagnostic().contains("adapter and release gate"));

            let unavailable = assess_macos_hvf(platform, Some(false));
            assert!(!unavailable.ready_for_adapter);
            assert_eq!(
                unavailable.blocker,
                Some(MacOsHvfBlocker::HypervisorUnavailable)
            );
            assert!(!unavailable.release_enabled);
            assert!(unavailable.diagnostic().contains("Hypervisor.framework"));

            let unknown = assess_macos_hvf(platform, None);
            assert!(!unknown.ready_for_adapter);
            assert_eq!(unknown.blocker, Some(MacOsHvfBlocker::ProbeUnavailable));
            assert!(!unknown.release_enabled);
            assert!(unknown.diagnostic().contains("could not be verified"));
        }
    }

    #[test]
    fn hvf_readiness_rejects_non_macos_hosts_even_when_fixture_says_supported() {
        for platform in [
            ComputerHostPlatformV1::LinuxX86_64,
            ComputerHostPlatformV1::WindowsX64,
            ComputerHostPlatformV1::WindowsArm64,
            ComputerHostPlatformV1::Unsupported,
        ] {
            let readiness = assess_macos_hvf(platform, Some(true));
            assert!(!readiness.ready_for_adapter);
            assert_eq!(
                readiness.blocker,
                Some(MacOsHvfBlocker::UnsupportedArchitecture)
            );
            assert!(!readiness.release_enabled);
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod native_tests {
    #[test]
    fn native_hvf_probe_never_opens_the_release_gate() {
        let readiness = super::probe_macos_hvf();
        assert!(!readiness.release_enabled);
        if readiness.blocker.is_some() {
            assert!(!readiness.ready_for_adapter);
        }
    }
}
