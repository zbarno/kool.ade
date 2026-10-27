use super::*;
use serde::{Deserialize, Serialize};

impl PlanEffect {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::RecheckPrerequisites => "Run the typed prerequisite check again",
            Self::OpenCodingToolsSettings => "Open the existing coding tool settings",
            Self::DownloadLockedImage => "Download the exact image named by the bundled lock",
            Self::RunIsolatedImageSmokeTest => "Run a bounded, isolated image smoke test",
            Self::ConfigureDockerAptRepository => {
                "Create only the missing `/etc/apt/keyrings` directory and pinned Docker signing key plus the exact distro-specific Debian or Ubuntu stable/amd64 APT source files. Use exclusive file creation, refuse conflicting administrator files, and verify the pinned Docker key before updating package metadata."
            }
            Self::ConfigureDockerDnfRepository => {
                "Create only the missing pinned Docker signing key and stable Fedora 45 x86_64 DNF repository files. Use exclusive file creation, refuse conflicting administrator files, and verify Docker's pinned RPM key fingerprint before refreshing package metadata."
            }
            Self::InstallLinuxRootlessPackages => {
                "Configure only the exact recipe's missing signed Docker repository files, refresh package metadata, then install Docker Engine and rootless prerequisites. APT-family package scripts may activate the system rootful service and require separate consent; Fedora package installation does not start it. Do not upgrade or remove packages."
            }
            Self::ConfigureLinuxSubordinateIds => {
                "Add a 65,536-ID subordinate UID/GID range for the current user only when needed; preserve existing and other users' entries."
            }
            Self::VerifyRootlesskitBeforeInstall => {
                "Require a fresh successful `rootlesskit true` check as the current user before install; abort on failure."
            }
            Self::RefuseConflictingExistingRootlessInstallation => {
                "Abort if another rootless daemon, service unit, or runtime state already exists; do not replace or start an existing rootless installation."
            }
            Self::InstallAndEnableCurrentUserRootlessDockerService => {
                "Create Kool.ad/e's fixed per-user unit with `ExecStart=/usr/bin/dockerd-rootless.sh` and `WantedBy=default.target`; start it only when inactive and enable it. No setup-tool `--force` command is used."
            }
            Self::PreserveSystemRootfulDockerService => {
                "Leave the system rootful Docker service, socket, and configuration untouched; never stop, disable, or modify them."
            }
        }
    }
}

impl ManagedFileChange {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::DockerUserConfiguration => "Docker user configuration",
            Self::DockerAptKeyringDirectory => {
                "`/etc/apt/keyrings`, created only when absent with root ownership and APT-readable directory permissions."
            }
            Self::DockerAptSigningKey => {
                "`/etc/apt/keyrings/docker.asc`, created only when absent, root-owned and readable by APT's download sandbox, pinned to Docker key fingerprint `9DC858229FC7DD38854AE2D88D81803C0EBFCD88`."
            }
            Self::DockerAptSourcesEntry => {
                "`/etc/apt/sources.list.d/koolade-docker.sources`, created only when absent for the exact Debian trixie or Ubuntu noble stable amd64 recipe with `Signed-By: /etc/apt/keyrings/docker.asc`."
            }
            Self::DockerDnfSigningKey => {
                "`/etc/pki/rpm-gpg/RPM-GPG-KEY-koolade-docker`, created only when absent and pinned to Docker's published RPM key fingerprint."
            }
            Self::DockerDnfRepositoryEntry => {
                "`/etc/yum.repos.d/koolade-docker-ce.repo`, created only when absent for Fedora 45 x86_64 stable packages with `gpgcheck=1`."
            }
            Self::LinuxSubordinateIdMappings => {
                "Current user's entries in `/etc/subuid` and `/etc/subgid`; preserve all other entries."
            }
            Self::CurrentUserRootlessDockerUnit => {
                "Current user's `~/.config/systemd/user/koolade-rootless-docker.service`; a custom `XDG_CONFIG_HOME` is unsupported and leaves this task manual. The fixed template sets `PATH=/usr/bin:/sbin:/usr/sbin:/bin`, `ExecStart=/usr/bin/dockerd-rootless.sh`, `Delegate=yes`, `Type=notify`, `Restart=always`, and `WantedBy=default.target`; create only when absent."
            }
        }
    }
}

impl ManagedServiceChange {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::DockerUserService => "Docker user service",
            Self::CurrentUserRootlessDockerService => {
                "Current user's systemd `koolade-rootless-docker.service`; start only if inactive and enable it."
            }
            Self::SystemRootfulDockerServicePackageSideEffect => {
                "Docker Engine package scripts may install, enable, or start the system rootful Docker service; Kool.ad/e will not stop, disable, or change its configuration."
            }
        }
    }
}

impl ManagedPackageChange {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Debian => {
                "Debian 13 amd64: Docker Engine, CLI, containerd, and rootless extras candidates must come from Docker's signed `trixie/stable` repository; `uidmap` and `dbus-user-session` must come from signed Debian trixie archives."
            }
            Self::Ubuntu => {
                "Ubuntu 24.04 amd64: `docker-ce`, `docker-ce-cli`, `containerd.io`, and `docker-ce-rootless-extras` candidates must come from the signed Docker `noble/stable` repository; `uidmap` and `dbus-user-session` use signed official Ubuntu Noble archives. The Pop!_OS 24.04 recipe uses the same Noble package base."
            }
            Self::PopOs => {
                "Pop!_OS 24.04 amd64: Docker Engine, CLI, containerd, and rootless extras candidates must come from Docker's signed Ubuntu `noble/stable` repository; `uidmap` and `dbus-user-session` must come from signed Ubuntu Noble archives."
            }
            Self::Fedora => {
                "Fedora 45 x86_64: Docker Engine, CLI, containerd, and rootless extras candidates must come from Docker's signed Fedora `45/x86_64/stable` repository; `shadow-utils` and `slirp4netns` must come from signed Fedora 45 archives."
            }
        }
    }
}

impl PlanConsent {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::RootfulDockerPackageActivation => {
                "Allow Docker Engine package installation to install, enable, or start the system rootful Docker service if Docker's package scripts request it. Kool.ad/e will not stop, disable, or alter that service."
            }
            Self::RootfulDockerCoexistence => {
                "Allow rootless Docker to coexist with an existing system rootful Docker daemon. The rootful service, socket, and configuration remain untouched."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RollbackGuidance {
    NoMutation,
    KeepVerifiedArtifactForRetry,
    ManualRecoveryRequired,
    KeepInstalledPackagesForManualRecovery,
    KeepSubordinateIdMappingsForManualRecovery,
    KeepRootlessUserStateForManualRecovery,
}

impl PrivilegeLevel {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::CurrentUser => "Runs as the current user; no elevation prompt",
            Self::Administrator => "Requires a separate OS administrator prompt",
        }
    }
}

impl ArtifactSource {
    pub(crate) fn description(&self) -> String {
        match self {
            Self::BundledContainerImage { reference } => {
                format!("bundled container image {reference}")
            }
            Self::VendorInstaller { publisher, product } => {
                format!("{publisher:?} installer for {product:?}")
            }
            Self::SignedPackageRepository {
                repository,
                product,
            } => {
                format!("{repository:?} package repository for {product:?}")
            }
        }
    }
}

impl ArtifactTrust {
    pub(crate) fn description(&self) -> String {
        match self {
            Self::DigestPinned { sha256 } => format!("SHA-256 {sha256}"),
            Self::PublisherSigned { sha256, publisher } => {
                format!("SHA-256 {sha256}, signed by {publisher:?}")
            }
        }
    }
}

impl RollbackGuidance {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::NoMutation => "No host configuration change is planned.",
            Self::KeepVerifiedArtifactForRetry => {
                "Keep the verified image for a later retry; this operation changes no host configuration."
            }
            Self::ManualRecoveryRequired => {
                "Stop automatic retries and inspect the listed package, file, and service changes before another repair. Do not stop, disable, or modify the system rootful Docker service."
            }
            Self::KeepInstalledPackagesForManualRecovery => {
                "Do not purge or downgrade packages automatically after failure; preserve package-manager state and the verified Docker source/key files for manual recovery."
            }
            Self::KeepSubordinateIdMappingsForManualRecovery => {
                "Do not remove or rewrite `/etc/subuid` or `/etc/subgid` automatically after failure; preserve all users' mappings and require manual recovery."
            }
            Self::KeepRootlessUserStateForManualRecovery => {
                "Do not remove or disable the app-owned rootless unit, delete user Docker data, or change the system rootful daemon after failure; preserve state for manual recovery."
            }
        }
    }
}
