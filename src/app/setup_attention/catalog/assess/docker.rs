use crate::execution::docker::environment::{
    DockerEnvironmentState as State, DockerEnvironmentStatus,
};

use super::super::Finding;

pub(super) fn finding(docker: &DockerEnvironmentStatus) -> Option<Finding> {
    Some(match docker.state {
        State::NotInstalled => Finding::action(
            "setup:docker:install",
            "Install a safe container runtime",
            "Kool.ad/e cannot find a supported local Docker runtime.",
            "Follow the supported Docker installation task for this operating system.",
        ),
        State::NotRunning => Finding::action(
            "setup:docker:start",
            "Start the container service",
            "The Docker service or Linux VM is not running.",
            "Start the verified local container service, then check again.",
        ),
        State::PermissionDenied => Finding::action(
            "setup:docker:permission",
            "Allow access to the container service",
            "Kool.ad/e cannot access the selected local Docker service.",
            "Check the service owner and supported user permissions.",
        ),
        State::RootfulLinuxRejected => Finding::action(
            "setup:docker:rootless",
            "Configure secure Docker",
            "Native Linux Docker is running with elevated daemon privileges.",
            "Configure the current user's rootless Docker Engine and verify its socket.",
        ),
        State::WindowsContainersRequired => Finding::action(
            "setup:docker:windows-containers",
            "Switch Docker to Linux containers",
            "Docker is currently serving Windows containers, which Kool.ad/e cannot use.",
            "Switch Docker Desktop to Linux containers, then retry the setup scan.",
        ),
        State::InsufficientDisk => Finding::action(
            "setup:docker:disk",
            "Free space for the execution runtime",
            "The Docker host does not have the required free disk space.",
            "Free disk space on the Docker host, then retry the readiness scan.",
        ),
        State::LimitsUnavailable => Finding::unverified(
            "setup:docker:limits",
            "Verify Docker resource limits",
            "Docker did not confirm the CPU, memory, and process limits required by the runtime.",
            "Review Docker Engine resource settings and retry the readiness scan.",
        ),
        State::CgroupsUnavailable => Finding::unverified(
            "setup:docker:cgroups",
            "Verify Docker cgroup support",
            "Docker did not report a supported cgroup version and driver.",
            "Use a Linux container Engine with cgroup v1 or v2 and a supported driver.",
        ),
        State::SeccompUnavailable => Finding::action(
            "setup:docker:seccomp",
            "Enable the required seccomp security profile",
            "Docker did not confirm the seccomp protection required by the execution runtime.",
            "Enable the default seccomp profile for Linux containers, then retry.",
        ),
        State::UnsupportedApi => Finding::unverified(
            "setup:docker:api",
            "Update Docker Engine compatibility",
            "Docker Engine's API version is outside Kool.ad/e's supported range.",
            "Update Docker Engine to a supported version, then retry.",
        ),
        State::UnsupportedOs => Finding::unverified(
            "setup:docker:os",
            "Use a Linux container engine",
            "The selected Docker Engine does not provide a supported Linux host.",
            "Use a Docker Engine that supports Linux containers.",
        ),
        State::UnsupportedArchitecture => Finding::unverified(
            "setup:docker:architecture",
            "Use a supported Docker architecture",
            "Docker Engine reported an unsupported runtime architecture.",
            "Use linux/amd64 or linux/arm64 for the Docker Engine and runtime image.",
        ),
        State::WorkspaceUnavailable => Finding::unverified(
            "setup:docker:workspace",
            "Repair application-owned Docker staging",
            "Kool.ad/e could not safely prepare its private Docker staging directory.",
            "Check the application state directory permissions and retry.",
        ),
        State::ProfileUnsupported => Finding::unverified(
            "setup:docker:profile",
            "Choose a supported runtime profile",
            "The requested runtime profile is not supported by this Docker Engine.",
            "Choose a supported project profile, then retry.",
        ),
        State::ApprovedImageUncheckable => Finding::unverified(
            "setup:docker:image-unverified",
            "Verify approved runtime images",
            "Docker did not verify the approved runtime image inventory.",
            "Check Docker image access and retry the read-only machine setup scan.",
        ),
        State::ApprovedImageDigestMismatch => Finding {
            status: super::super::FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:docker:image-digest",
                "Verify the approved runtime image digest",
                "The observed image does not match the application-approved digest.",
                "Use only the exact image reference from the bundled application lock.",
            )
        },
        State::ApprovedImageArchitectureMismatch => Finding {
            status: super::super::FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:docker:image-architecture",
                "Verify runtime image architecture",
                "The approved runtime image architecture does not match Docker Engine.",
                "Use an approved image built for the detected Engine architecture.",
            )
        },
        State::DesktopVmRequired => Finding::action(
            "setup:docker:desktop",
            "Start Docker Desktop's Linux environment",
            "Docker Desktop's local Linux backend is unavailable.",
            "Install or start Docker Desktop with its supported Linux container backend.",
        ),
        State::UntrustedEndpoint => Finding::action(
            "setup:docker:endpoint",
            "Connect to trusted local Docker",
            "The selected Docker endpoint cannot be trusted.",
            "Choose the locally verified Docker Engine or Docker Desktop connection.",
        ),
        State::CapabilitiesIncompatible => Finding::unverified(
            "setup:docker:capabilities",
            "Resolve Docker compatibility",
            "The Docker engine did not pass the required security or resource checks.",
            "Review the specific Engine diagnostic before changing settings.",
        ),
        State::MissingApprovedImage | State::Ready => return None,
    })
}
