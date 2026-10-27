use crate::app::setup_attention::catalog::{Finding, FindingStatus};
use crate::execution::docker::environment::DockerEnvironmentState as State;

/// User-facing macOS steps stay in Finder and Docker Desktop. A ready socket
/// alone does not certify the VM or make Desktop execution supported.
pub(super) fn finding(state: State) -> Option<Finding> {
    Some(match state {
        State::NotInstalled | State::DesktopVmRequired => Finding::action(
            "setup:docker:desktop",
            "Install or start Docker Desktop",
            "Kool.ad/e cannot find Docker Desktop's local Linux environment.",
            "Install the Intel or Apple silicon build from Docker's official macOS guide. Open Docker Desktop from Applications, review its license and macOS security prompts, and wait for the Linux environment to start. Do not bypass Gatekeeper or change system security settings.",
        ),
        State::NotRunning => Finding::action(
            "setup:docker:start",
            "Start Docker Desktop",
            "Docker Desktop's Linux environment is not running.",
            "Open Docker Desktop from Applications or the Dock. Complete any first-run or update prompts and wait until Docker Desktop reports that it is running, then check again.",
        ),
        State::PermissionDenied => Finding::action(
            "setup:docker:permission",
            "Review Docker Desktop access",
            "Kool.ad/e cannot access Docker Desktop's current-user connection.",
            "Open Docker Desktop and review its own access prompts. Keep the connection local to your macOS account; do not grant broad socket access or switch to a remote Docker endpoint.",
        ),
        State::InsufficientDisk => Finding::action(
            "setup:docker:disk",
            "Free space in Docker Desktop",
            "Docker Desktop's Linux environment does not have enough verified free storage.",
            "Review Docker Desktop Settings and the Resources disk image location and size. Return here and let Kool.ad/e remeasure VM capacity; Mac host free space alone does not verify available VM storage.",
        ),
        State::LimitsUnavailable => Finding::unverified(
            "setup:docker:limits",
            "Verify Docker Desktop resources",
            "Kool.ad/e could not verify the CPU, memory, or process limits in Docker Desktop's Linux environment.",
            "Review Docker Desktop's Resources settings and retry. Unknown VM limits keep repository execution blocked.",
        ),
        State::UnsupportedArchitecture | State::ApprovedImageArchitectureMismatch => Finding {
            status: FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:docker:architecture",
                "Verify the Mac and image architecture",
                "The Mac architecture and Docker Desktop's approved Linux image do not match a verified supported combination.",
                "Use the official Docker Desktop build for this Mac and an architecture-matched approved image. Rosetta or emulation does not establish support; retry after the runtime architecture is verified.",
            )
        },
        State::UntrustedEndpoint => Finding {
            status: FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:docker:endpoint",
                "Reconnect to local Docker Desktop",
                "Kool.ad/e could not verify Docker Desktop's publisher or confirm the selected connection is this Mac's local socket.",
                "If Docker Desktop's publisher check failed, install a fresh copy from the official Mac guide. Otherwise select its local Linux context. Do not bypass macOS security warnings or use TCP, SSH, or a remote context.",
            )
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_desktop_shows_official_no_terminal_install_steps() {
        let finding = finding(State::DesktopVmRequired).expect("Desktop setup task");
        assert_eq!(finding.id, "setup:docker:desktop");
        assert!(finding.next_step.contains("official macOS guide"));
        assert!(finding.next_step.contains("Applications"));
        assert!(finding.next_step.contains("Do not bypass Gatekeeper"));
        assert!(!finding.next_step.contains("Homebrew"));
    }

    #[test]
    fn host_storage_does_not_replace_verified_vm_capacity() {
        let finding = finding(State::InsufficientDisk).expect("disk task");
        assert!(finding.explanation.contains("Linux environment"));
        assert!(finding.next_step.contains("Mac host free space alone"));
    }

    #[test]
    fn untrusted_endpoints_stay_blocked_and_local() {
        let finding = finding(State::UntrustedEndpoint).expect("endpoint task");
        assert_eq!(finding.status, FindingStatus::Unsupported);
        assert!(finding.next_step.contains("TCP, SSH"));
    }
}
