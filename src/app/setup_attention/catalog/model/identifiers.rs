use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Linux,
    MacOs,
    Windows,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxDistributionV1 {
    Ubuntu,
    Debian,
    Fedora,
    Arch,
    Suse,
    Alpine,
    PopOs,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LinuxDistributionVersionV1 {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LinuxKernelVersionV1 {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxPackageManagerV1 {
    AptGet,
    Dnf,
    Pacman,
    Zypper,
    Apk,
}

impl LinuxPackageManagerV1 {
    pub fn for_distribution(distribution: LinuxDistributionV1) -> Option<Self> {
        match distribution {
            LinuxDistributionV1::Ubuntu
            | LinuxDistributionV1::Debian
            | LinuxDistributionV1::PopOs => Some(Self::AptGet),
            LinuxDistributionV1::Fedora => Some(Self::Dnf),
            LinuxDistributionV1::Arch => Some(Self::Pacman),
            LinuxDistributionV1::Suse => Some(Self::Zypper),
            LinuxDistributionV1::Alpine => Some(Self::Apk),
            LinuxDistributionV1::Other => None,
        }
    }
}

/// An exact native-host recipe candidate. A candidate does not imply a
/// product support claim; VM validation is still required before installation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxRootlessRecipeV1 {
    Debian13Amd64,
    Ubuntu2404Amd64,
    PopOs2404Amd64,
    Fedora45Amd64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxRootlessMechanismV1 {
    SystemdUser,
    Unknown,
}

impl HostKind {
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Unsupported
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrerequisiteDomain {
    Host,
    Docker,
    Provider,
    Image,
    Git,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrerequisiteCapability {
    HostFamily,
    HostEdition,
    HostArchitecture,
    LinuxDistribution,
    LinuxRootlessRecipe,
    LinuxKernel,
    LinuxUserNamespaces,
    LinuxSubuid,
    LinuxSubgid,
    LinuxRootlessHelpers,
    LinuxCgroups,
    LinuxUidMappings,
    LinuxServiceManager,
    LinuxPackageManager,
    Wsl2,
    Virtualization,
    DockerEngine,
    DockerEndpoint,
    DockerRootless,
    DockerDesktopVm,
    DockerDesktopTrust,
    DockerCgroups,
    DockerCapacity,
    DockerSecurity,
    DockerRootlessSmoke,
    ProviderExecutable,
    ProviderVersion,
    ProviderAuthentication,
    ProviderModels,
    ProviderContextOnly,
    ApprovedImage,
    GitExecutable,
    GitIdentity,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderIdV1 {
    Pi,
    Codex,
    Claude,
    OpenCode,
    Copilot,
    Antigravity,
}

impl ProviderIdV1 {
    pub fn from_probe_id(id: &str) -> Option<Self> {
        match id {
            "pi" => Some(Self::Pi),
            "codex" => Some(Self::Codex),
            "claude" => Some(Self::Claude),
            "opencode" => Some(Self::OpenCode),
            "copilot" => Some(Self::Copilot),
            "antigravity" => Some(Self::Antigravity),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageProfileV1 {
    General,
    Rust,
    Node,
    Python,
    Go,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageArchitectureV1 {
    Amd64,
    Arm64,
    Unsupported,
    Unknown,
}

impl ImageArchitectureV1 {
    pub fn from_platform_arch(arch: &str) -> Self {
        match arch {
            "amd64" | "x86_64" => Self::Amd64,
            "arm64" | "aarch64" => Self::Arm64,
            "" => Self::Unknown,
            _ => Self::Unsupported,
        }
    }

    pub fn image_name(self) -> Option<&'static str> {
        match self {
            Self::Amd64 => Some("amd64"),
            Self::Arm64 => Some("arm64"),
            Self::Unsupported | Self::Unknown => None,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub enum PrerequisiteSubject {
    None,
    Provider(ProviderIdV1),
    Image {
        profile: ImageProfileV1,
        architecture: ImageArchitectureV1,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrerequisiteId {
    pub domain: PrerequisiteDomain,
    pub platform: HostKind,
    pub capability: PrerequisiteCapability,
    pub subject: PrerequisiteSubject,
}
