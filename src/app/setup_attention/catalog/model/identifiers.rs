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
pub enum WindowsEditionV1 {
    Home,
    Pro,
    Enterprise,
    Education,
    Server,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsProductV1 {
    Windows10,
    Windows11,
    Server,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WindowsKernelVersionV1 {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: Option<u16>,
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
    WindowsWslFeature,
    WindowsVmPlatformFeature,
    WindowsWslKernel,
    WindowsReboot,
    WindowsWslPolicy,
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

    pub fn probe_id(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
            Self::Copilot => "copilot",
            Self::Antigravity => "antigravity",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Pi => "Pi",
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::OpenCode => "OpenCode",
            Self::Copilot => "GitHub Copilot CLI",
            Self::Antigravity => "Google Antigravity CLI",
        }
    }

    pub fn installation_guide(self) -> &'static str {
        match self {
            Self::Pi => "https://pi.dev/docs/latest/quickstart",
            Self::Codex => "https://learn.chatgpt.com/docs/codex/cli",
            Self::Claude => "https://code.claude.com/docs/en/setup",
            Self::OpenCode => "https://opencode.ai/v2/docs/",
            Self::Copilot => "https://docs.github.com/en/copilot/get-started/cli-quickstart",
            Self::Antigravity => "https://antigravity.google/docs/getting-started",
        }
    }

    pub fn authentication_guide(self) -> &'static str {
        match self {
            Self::Pi => "https://pi.dev/docs/latest/providers",
            Self::Codex => "https://learn.chatgpt.com/docs/auth",
            Self::Claude => "https://code.claude.com/docs/en/authentication",
            Self::OpenCode => "https://dev.opencode.ai/v2/docs/cli/providers",
            Self::Copilot => {
                "https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli"
            }
            Self::Antigravity => "https://antigravity.google/docs/cli/install/",
        }
    }

    pub fn authentication_steps(self) -> &'static [&'static str] {
        match self {
            Self::Pi => &[
                "Open Pi in a terminal and enter `/login`.",
                "Choose a provider and use its browser or device sign-in when offered. If Pi asks for an API key, enter it only in Pi.",
                "Return here and choose Check again. Kool.ad/e never reads Pi's credential file.",
            ],
            Self::Codex => &[
                "Open Codex in a terminal by running `codex`.",
                "Choose Sign in with ChatGPT or another method offered by Codex, then complete the sign-in in its browser flow.",
                "Return here and choose Check again. Keep one-time codes and credentials in Codex and its browser.",
            ],
            Self::Claude => &[
                "Start Claude Code in a terminal by running `claude` and follow its browser sign-in prompt.",
                "If the browser does not open, copy the login URL from Claude's terminal. Enter any one-time code only in Claude's terminal.",
                "Return here and choose Check again. Kool.ad/e does not capture browser or terminal credentials.",
            ],
            Self::OpenCode => &[
                "Run `opencode auth login` in a terminal and choose your provider.",
                "Use its browser or device sign-in when offered. If you choose API-key entry, enter the key only in OpenCode's own prompt.",
                "Return here and choose Check again. Do not copy tokens or keys into Kool.ad/e.",
            ],
            Self::Copilot => &[
                "Open GitHub Copilot CLI in a terminal by running `copilot`.",
                "At its prompt, enter `/login` and follow the browser or one-time device-code flow.",
                "Complete approval on GitHub, return to Copilot CLI, then choose Check again here.",
            ],
            Self::Antigravity => &[
                "Start Antigravity CLI in a terminal by running `agy`.",
                "If no local sign-in is saved, it opens your browser. Complete Google sign-in there; for a remote terminal, follow the URL and code steps it shows.",
                "Return here and choose Check again. Keep credentials, one-time codes, and browser data out of Kool.ad/e.",
            ],
        }
    }

    pub fn model_guide(self) -> &'static str {
        match self {
            Self::Pi => "https://pi.dev/docs/latest/models",
            Self::Codex => "https://learn.chatgpt.com/docs/codex/cli",
            Self::Claude => "https://code.claude.com/docs/en/model-config",
            Self::OpenCode => "https://dev.opencode.ai/v2/docs/cli/providers",
            Self::Copilot => {
                "https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference"
            }
            Self::Antigravity => "https://antigravity.google/docs/models/",
        }
    }

    pub fn install_note(self, host: HostKind) -> &'static str {
        super::install_channels::install_note(self, host)
    }

    pub fn install_channel_note(self, host: HostKind) -> &'static str {
        super::install_channels::install_channel_note(self, host)
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
