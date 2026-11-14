#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerHostPlatformV1 {
    LinuxX86_64,
    LinuxAarch64,
    MacOsIntel,
    MacOsAppleSilicon,
    WindowsX64,
    WindowsArm64,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComputerGuestArchitectureRequirementV1 {
    X86_64,
    Aarch64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComputerHostSupportBlockerV1 {
    NativeBackendNotImplemented,
    ApprovedGuestImageUnavailable,
    HostReleaseGateClosed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ComputerHostCompatibilityV1 {
    pub(crate) guest_architecture: ComputerGuestArchitectureRequirementV1,
    pub(crate) blockers: &'static [ComputerHostSupportBlockerV1],
}

impl ComputerHostPlatformV1 {
    pub fn current() -> Self {
        Self::from_os_arch(std::env::consts::OS, native_architecture())
    }

    fn from_os_arch(os: &str, architecture: &str) -> Self {
        match (os, architecture) {
            ("linux", "x86_64") => Self::LinuxX86_64,
            ("linux", "aarch64") => Self::LinuxAarch64,
            ("macos", "x86_64") => Self::MacOsIntel,
            ("macos", "aarch64") => Self::MacOsAppleSilicon,
            ("windows", "x86_64") => Self::WindowsX64,
            ("windows", "aarch64") => Self::WindowsArm64,
            _ => Self::Unsupported,
        }
    }

    /// Hypervisor family to probe for this host. This selects a candidate only;
    /// readiness still requires the platform adapter's own verified resource,
    /// storage, process, image, and transport controls.
    pub fn candidate_qemu_hypervisor(self) -> Option<crate::computer_use::ComputerHypervisorV1> {
        use crate::computer_use::ComputerHypervisorV1 as Hypervisor;

        match self {
            Self::LinuxX86_64 => Some(Hypervisor::QemuKvm),
            Self::MacOsIntel | Self::MacOsAppleSilicon => Some(Hypervisor::QemuHvf),
            Self::WindowsX64 => Some(Hypervisor::QemuWhpx),
            Self::LinuxAarch64 | Self::WindowsArm64 | Self::Unsupported => None,
        }
    }

    /// Report the native-architecture guest requirement and why this macOS
    /// target remains unavailable. This never selects an image or starts a VM.
    pub(crate) fn macos_compatibility(self) -> Option<ComputerHostCompatibilityV1> {
        match self {
            Self::MacOsIntel => Some(ComputerHostCompatibilityV1 {
                guest_architecture: ComputerGuestArchitectureRequirementV1::X86_64,
                blockers: &[
                    ComputerHostSupportBlockerV1::NativeBackendNotImplemented,
                    ComputerHostSupportBlockerV1::HostReleaseGateClosed,
                ],
            }),
            Self::MacOsAppleSilicon => Some(ComputerHostCompatibilityV1 {
                guest_architecture: ComputerGuestArchitectureRequirementV1::Aarch64,
                blockers: &[
                    ComputerHostSupportBlockerV1::ApprovedGuestImageUnavailable,
                    ComputerHostSupportBlockerV1::NativeBackendNotImplemented,
                    ComputerHostSupportBlockerV1::HostReleaseGateClosed,
                ],
            }),
            _ => None,
        }
    }
}

impl ComputerHostCompatibilityV1 {
    pub(crate) fn diagnostic(self) -> &'static str {
        if self.guest_architecture == ComputerGuestArchitectureRequirementV1::Aarch64
            && self
                .blockers
                .contains(&ComputerHostSupportBlockerV1::ApprovedGuestImageUnavailable)
        {
            "macOS Apple Silicon Computer Use is unavailable until an approved aarch64 guest image, native backend, and host release gate are ready."
        } else {
            "macOS Intel Computer Use is unavailable until its native backend and host release gate are ready."
        }
    }
}

#[cfg(target_os = "macos")]
fn native_architecture() -> &'static str {
    let mut translated = 0_i32;
    let mut length = std::mem::size_of_val(&translated);
    // Apple documents `sysctl.proc_translated` as the way to identify an
    // x86_64 process running under Rosetta. Such a process still runs on an
    // Apple Silicon host, whose VM acceleration and guest architecture differ.
    let status = unsafe {
        libc::sysctlbyname(
            c"sysctl.proc_translated".as_ptr(),
            (&mut translated as *mut i32).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status == 0 {
        return macos_architecture_from_probe(std::env::consts::ARCH, Ok(translated));
    }
    macos_architecture_from_probe(
        std::env::consts::ARCH,
        Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO)),
    )
}

#[cfg(any(target_os = "macos", test))]
fn macos_architecture_from_probe(
    target_architecture: &str,
    result: Result<i32, i32>,
) -> &'static str {
    match result {
        Ok(0) | Err(libc::ENOENT) => match target_architecture {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            _ => "unknown",
        },
        Ok(1) => "aarch64",
        _ => "unknown",
    }
}

#[cfg(target_os = "windows")]
fn native_architecture() -> &'static str {
    use windows_sys::Win32::System::SystemInformation::{
        GetNativeSystemInfo, PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_ARM64,
        PROCESSOR_ARCHITECTURE_INTEL, SYSTEM_INFO,
    };

    let mut info = SYSTEM_INFO::default();
    unsafe { GetNativeSystemInfo(&mut info) };
    let architecture = unsafe { info.Anonymous.Anonymous.wProcessorArchitecture };
    match architecture {
        PROCESSOR_ARCHITECTURE_AMD64 => "x86_64",
        PROCESSOR_ARCHITECTURE_ARM64 => "aarch64",
        PROCESSOR_ARCHITECTURE_INTEL => "x86",
        _ => "unknown",
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn native_architecture() -> &'static str {
    std::env::consts::ARCH
}

#[cfg(test)]
mod tests {
    use super::ComputerHostPlatformV1 as Platform;

    #[test]
    fn platform_detection_keeps_os_architecture_pairs_distinct() {
        assert_eq!(
            Platform::from_os_arch("macos", "x86_64"),
            Platform::MacOsIntel
        );
        assert_eq!(
            Platform::from_os_arch("macos", "aarch64"),
            Platform::MacOsAppleSilicon
        );
        assert_eq!(
            Platform::from_os_arch("windows", "x86_64"),
            Platform::WindowsX64
        );
        assert_eq!(
            Platform::from_os_arch("linux", "aarch64"),
            Platform::LinuxAarch64
        );
        assert_eq!(
            Platform::from_os_arch("freebsd", "x86_64"),
            Platform::Unsupported
        );
    }

    #[test]
    fn rosetta_process_architecture_reports_the_native_apple_silicon_host() {
        assert_eq!(
            super::macos_architecture_from_probe("x86_64", Ok(1)),
            "aarch64"
        );
        assert_eq!(
            super::macos_architecture_from_probe("x86_64", Ok(0)),
            "x86_64"
        );
        assert_eq!(
            super::macos_architecture_from_probe("x86_64", Err(libc::ENOENT)),
            "x86_64"
        );
        assert_eq!(
            super::macos_architecture_from_probe("x86_64", Err(libc::EACCES)),
            "unknown"
        );
    }

    #[test]
    fn qemu_accelerator_candidates_are_host_specific_and_not_readiness_claims() {
        use crate::computer_use::ComputerHypervisorV1 as Hypervisor;

        assert_eq!(
            Platform::LinuxX86_64.candidate_qemu_hypervisor(),
            Some(Hypervisor::QemuKvm)
        );
        assert_eq!(
            Platform::MacOsIntel.candidate_qemu_hypervisor(),
            Some(Hypervisor::QemuHvf)
        );
        assert_eq!(
            Platform::MacOsAppleSilicon.candidate_qemu_hypervisor(),
            Some(Hypervisor::QemuHvf)
        );
        assert_eq!(
            Platform::WindowsX64.candidate_qemu_hypervisor(),
            Some(Hypervisor::QemuWhpx)
        );
        assert_eq!(Platform::WindowsArm64.candidate_qemu_hypervisor(), None);
        assert_eq!(Platform::Unsupported.candidate_qemu_hypervisor(), None);
    }

    #[test]
    fn macos_selects_a_native_guest_architecture_without_enabling_the_backend() {
        use super::{
            ComputerGuestArchitectureRequirementV1 as Guest,
            ComputerHostSupportBlockerV1 as Blocker,
        };

        let intel = Platform::MacOsIntel.macos_compatibility().unwrap();
        assert_eq!(intel.guest_architecture, Guest::X86_64);
        assert_eq!(
            intel.blockers,
            &[
                Blocker::NativeBackendNotImplemented,
                Blocker::HostReleaseGateClosed
            ]
        );

        let apple_silicon = Platform::MacOsAppleSilicon.macos_compatibility().unwrap();
        assert_eq!(apple_silicon.guest_architecture, Guest::Aarch64);
        assert_eq!(
            apple_silicon.blockers,
            &[
                Blocker::ApprovedGuestImageUnavailable,
                Blocker::NativeBackendNotImplemented,
                Blocker::HostReleaseGateClosed
            ]
        );
        assert_eq!(Platform::WindowsArm64.macos_compatibility(), None);
    }
}
