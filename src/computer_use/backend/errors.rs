use super::ComputerHostPlatformV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerBackendErrorV1 {
    UnsupportedHost,
    HostNotReady,
    InvalidSession,
    UntrustedSession,
    UnsupportedPolicy,
    UnsupportedTarget,
    InvalidOverlay,
    DuplicateSession,
    MachineSlotLimit,
    ResourceLimit,
    UnsafeRuntimeDirectory,
    OwnershipMismatch,
    StaleLease,
    InvalidTransition,
    QemuUnavailable,
    QemuStartFailed,
    QemuExited,
    QemuTimedOut,
    GuestChannelUnavailable,
    GuestReadinessUnverified,
    SnapshotNotAllowed,
    Cancelled,
    Io,
}

impl std::fmt::Display for ComputerBackendErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == Self::UnsupportedHost {
            write!(
                f,
                "computer-use backend unavailable: {}",
                unsupported_host_platform_diagnostic(ComputerHostPlatformV1::current())
            )
        } else {
            write!(f, "computer-use backend error: {self:?}")
        }
    }
}

fn unsupported_host_platform_diagnostic(platform: ComputerHostPlatformV1) -> &'static str {
    if let Some(compatibility) = platform.macos_compatibility() {
        return compatibility.diagnostic();
    }
    match platform {
        ComputerHostPlatformV1::LinuxX86_64 => unsupported_host_diagnostic("linux", "x86_64"),
        ComputerHostPlatformV1::LinuxAarch64 => unsupported_host_diagnostic("linux", "aarch64"),
        ComputerHostPlatformV1::MacOsIntel => unsupported_host_diagnostic("macos", "x86_64"),
        ComputerHostPlatformV1::MacOsAppleSilicon => {
            unsupported_host_diagnostic("macos", "aarch64")
        }
        ComputerHostPlatformV1::WindowsX64 => unsupported_host_diagnostic("windows", "x86_64"),
        ComputerHostPlatformV1::WindowsArm64 => unsupported_host_diagnostic("windows", "aarch64"),
        ComputerHostPlatformV1::Unsupported => {
            "this operating system and architecture are unsupported."
        }
    }
}

fn unsupported_host_diagnostic(os: &str, architecture: &str) -> &'static str {
    match (os, architecture) {
        ("linux", "x86_64") => {
            "Linux x86_64 requires verified KVM, delegated resource controls, and release checks."
        }
        ("linux", "aarch64") => {
            "Linux arm64 Computer Use is not enabled; only x86_64 Linux is verified."
        }
        ("macos", "x86_64") => {
            "macOS Intel Computer Use is not enabled; its adapter and release checks are incomplete."
        }
        ("macos", "aarch64") => {
            "macOS Apple Silicon Computer Use is not enabled; its adapter and release checks are incomplete."
        }
        ("windows", "x86_64") => {
            "Windows x64 Computer Use is not enabled; its adapter and release checks are incomplete."
        }
        ("windows", "aarch64") => "Windows ARM64 Computer Use is unsupported.",
        _ => "this operating system and architecture are unsupported.",
    }
}

impl std::error::Error for ComputerBackendErrorV1 {}

#[cfg(test)]
mod tests {
    use super::{
        ComputerBackendErrorV1, ComputerHostPlatformV1 as Platform, unsupported_host_diagnostic,
        unsupported_host_platform_diagnostic,
    };

    #[test]
    fn unsupported_host_errors_name_each_platform_gate() {
        for (os, architecture, expected) in [
            ("macos", "x86_64", "macOS Intel Computer Use is not enabled"),
            (
                "macos",
                "aarch64",
                "macOS Apple Silicon Computer Use is not enabled",
            ),
            (
                "windows",
                "x86_64",
                "Windows x64 Computer Use is not enabled",
            ),
        ] {
            assert!(unsupported_host_diagnostic(os, architecture).contains(expected));
        }
        assert!(
            ComputerBackendErrorV1::UnsupportedHost
                .to_string()
                .contains(unsupported_host_platform_diagnostic(Platform::current()))
        );
    }

    #[test]
    fn unsupported_host_diagnostics_use_native_host_architecture() {
        assert!(
            unsupported_host_platform_diagnostic(Platform::MacOsAppleSilicon)
                .contains("macOS Apple Silicon")
        );
        assert!(
            unsupported_host_platform_diagnostic(Platform::WindowsArm64).contains("Windows ARM64")
        );
        assert!(
            unsupported_host_platform_diagnostic(Platform::MacOsAppleSilicon)
                .contains("approved aarch64 guest image")
        );
    }
}
