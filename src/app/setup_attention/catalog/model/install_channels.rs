use super::identifiers::{
    HostKind, ImageArchitectureV1, LinuxDistributionV1, LinuxDistributionVersionV1, ProviderIdV1,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderInstallChannelV1 {
    ClaudeSignedBinary,
    AptLike,
    RpmLike,
    ArchLike,
    AlpineApk,
    MacHomebrew,
    MacPackage,
    WindowsWinget,
    WindowsMsi,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderInstallClassificationV1 {
    AppOwnedReviewedRecipe,
    OfficialVendorGuideOnly,
    UnsupportedCombination,
}

pub fn classify_provider_install_channel(
    provider: ProviderIdV1,
    host: HostKind,
    distribution: Option<LinuxDistributionV1>,
    version: Option<LinuxDistributionVersionV1>,
    architecture: ImageArchitectureV1,
    channel: ProviderInstallChannelV1,
) -> ProviderInstallClassificationV1 {
    use ProviderInstallChannelV1 as Channel;
    use ProviderInstallClassificationV1 as Classification;

    if channel == Channel::ClaudeSignedBinary {
        return if provider == ProviderIdV1::Claude
            && host == HostKind::Linux
            && distribution == Some(LinuxDistributionV1::Ubuntu)
            && version.is_some_and(|version| version.major == 24 && version.minor == 4)
            && architecture == ImageArchitectureV1::Amd64
        {
            Classification::AppOwnedReviewedRecipe
        } else {
            Classification::UnsupportedCombination
        };
    }

    match channel {
        Channel::AptLike if host == HostKind::Linux => {
            classify_linux_channel(distribution, |distribution| {
                matches!(
                    distribution,
                    LinuxDistributionV1::Ubuntu
                        | LinuxDistributionV1::Debian
                        | LinuxDistributionV1::PopOs
                )
            })
        }
        Channel::RpmLike if host == HostKind::Linux => {
            classify_linux_channel(distribution, |distribution| {
                matches!(
                    distribution,
                    LinuxDistributionV1::Fedora | LinuxDistributionV1::Suse
                )
            })
        }
        Channel::ArchLike if host == HostKind::Linux => {
            classify_linux_channel(distribution, |distribution| {
                distribution == LinuxDistributionV1::Arch
            })
        }
        Channel::AlpineApk if host == HostKind::Linux => {
            classify_linux_channel(distribution, |distribution| {
                distribution == LinuxDistributionV1::Alpine
            })
        }
        Channel::MacHomebrew | Channel::MacPackage if host == HostKind::MacOs => {
            Classification::OfficialVendorGuideOnly
        }
        Channel::WindowsWinget | Channel::WindowsMsi if host == HostKind::Windows => {
            Classification::OfficialVendorGuideOnly
        }
        Channel::Unknown
            if matches!(host, HostKind::Linux | HostKind::MacOs | HostKind::Windows) =>
        {
            Classification::OfficialVendorGuideOnly
        }
        _ => Classification::UnsupportedCombination,
    }
}

fn classify_linux_channel(
    distribution: Option<LinuxDistributionV1>,
    matches_channel: impl FnOnce(LinuxDistributionV1) -> bool,
) -> ProviderInstallClassificationV1 {
    use ProviderInstallClassificationV1 as Classification;

    match distribution {
        Some(LinuxDistributionV1::Other) | None => Classification::OfficialVendorGuideOnly,
        Some(distribution) if matches_channel(distribution) => {
            Classification::OfficialVendorGuideOnly
        }
        Some(_) => Classification::UnsupportedCombination,
    }
}

const LINUX_CHANNELS: &[ProviderInstallChannelV1] = &[
    ProviderInstallChannelV1::AptLike,
    ProviderInstallChannelV1::RpmLike,
    ProviderInstallChannelV1::ArchLike,
    ProviderInstallChannelV1::AlpineApk,
];
const MACOS_CHANNELS: &[ProviderInstallChannelV1] = &[
    ProviderInstallChannelV1::MacHomebrew,
    ProviderInstallChannelV1::MacPackage,
];
const WINDOWS_CHANNELS: &[ProviderInstallChannelV1] = &[
    ProviderInstallChannelV1::WindowsWinget,
    ProviderInstallChannelV1::WindowsMsi,
];

fn host_guidance_classification(
    provider: ProviderIdV1,
    host: HostKind,
) -> ProviderInstallClassificationV1 {
    let channels = match host {
        HostKind::Linux => LINUX_CHANNELS,
        HostKind::MacOs => MACOS_CHANNELS,
        HostKind::Windows => WINDOWS_CHANNELS,
        HostKind::Unsupported => return ProviderInstallClassificationV1::UnsupportedCombination,
    };
    let architecture = ImageArchitectureV1::from_platform_arch(std::env::consts::ARCH);
    let known_channels_are_manual = channels.iter().all(|channel| {
        classify_provider_install_channel(provider, host, None, None, architecture, *channel)
            == ProviderInstallClassificationV1::OfficialVendorGuideOnly
    });
    let unknown_channel_is_manual = classify_provider_install_channel(
        provider,
        host,
        None,
        None,
        architecture,
        ProviderInstallChannelV1::Unknown,
    ) == ProviderInstallClassificationV1::OfficialVendorGuideOnly;
    if known_channels_are_manual && unknown_channel_is_manual {
        ProviderInstallClassificationV1::OfficialVendorGuideOnly
    } else {
        ProviderInstallClassificationV1::UnsupportedCombination
    }
}

pub(super) fn install_channel_note(provider: ProviderIdV1, host: HostKind) -> &'static str {
    match (host_guidance_classification(provider, host), provider, host) {
        (ProviderInstallClassificationV1::UnsupportedCombination, _, HostKind::Unsupported) => {
            "Kool.ad/e has no verified install-channel guidance for this operating system. Use the provider's official documentation; no install command is inferred."
        }
        (_, ProviderIdV1::Claude, HostKind::Linux) => {
            "Only Claude Code's reviewed signed binary for Ubuntu 24.04 x86_64 is app-owned. When Anthropic's guide documents them, apt-like, rpm-like, Arch-like, Alpine, and unknown-distro channels remain official-guide/manual-only; Kool.ad/e does not infer package commands."
        }
        (_, ProviderIdV1::Claude, HostKind::MacOs) => {
            "When Anthropic's guide documents them, Homebrew and .pkg channels remain official-guide/manual-only; Kool.ad/e does not run vendor installers or infer package commands."
        }
        (_, ProviderIdV1::Claude, HostKind::Windows) => {
            "When Anthropic's guide documents them, winget and MSI channels remain official-guide/manual-only; Kool.ad/e does not run vendor installers or infer package commands."
        }
        (_, _, HostKind::Linux) => {
            "When the provider's official guide documents them, apt-like, rpm-like, Arch-like, Alpine, and unknown-distro channels are official-guide/manual-only. Kool.ad/e does not infer package commands."
        }
        (_, _, HostKind::MacOs) => {
            "When the provider's official guide documents them, Homebrew and .pkg channels are official-guide/manual-only. Kool.ad/e does not infer package commands."
        }
        (_, _, HostKind::Windows) => {
            "When the provider's official guide documents them, winget and MSI channels are official-guide/manual-only. Kool.ad/e does not infer package commands."
        }
        (_, _, HostKind::Unsupported) => unreachable!("unsupported hosts return above"),
    }
}

pub(super) fn install_note(provider: ProviderIdV1, host: HostKind) -> &'static str {
    if host_guidance_classification(provider, host)
        == ProviderInstallClassificationV1::UnsupportedCombination
    {
        return install_channel_note(provider, host);
    }

    match (provider, host) {
        (ProviderIdV1::Claude, HostKind::Windows) => {
            "Anthropic documents native Windows and WSL installation. Kool.ad/e's reviewed install recipe is limited to Ubuntu 24.04 x86_64."
        }
        (ProviderIdV1::Claude, HostKind::Linux) => {
            "A reviewed current-user recipe is available only on Ubuntu 24.04 x86_64. Other Linux hosts use Anthropic's official guide; avoid administrator-level global npm installs."
        }
        (ProviderIdV1::Codex, HostKind::Windows) => {
            "Follow OpenAI's current Windows/WSL support notes before installing; do not assume every Windows path is production-ready."
        }
        (ProviderIdV1::OpenCode, HostKind::Windows) => {
            "OpenCode recommends WSL for Windows; its current install options vary by release channel."
        }
        (ProviderIdV1::Copilot, HostKind::Windows) => {
            "GitHub documents a Windows package-manager option and a cross-platform npm option with a Node.js prerequisite."
        }
        (ProviderIdV1::Antigravity, _) => {
            "Google's installer is user-scoped, but Kool.ad/e does not execute the vendor's mutable shell or PowerShell installer."
        }
        (ProviderIdV1::OpenCode, _) => {
            "OpenCode has multiple release channels with different package names; confirm the intended channel in its official guide."
        }
        (ProviderIdV1::Pi, _) => {
            "Follow Pi's official user-level installation instructions. Kool.ad/e does not run vendor shell or package scripts."
        }
        (ProviderIdV1::Codex, _) => {
            "Use the official Codex CLI installation instructions for this operating system."
        }
        (ProviderIdV1::Claude, _) => {
            "Use Anthropic's documented installation path. Do not run a global package install with administrator privileges."
        }
        (ProviderIdV1::Copilot, _) => {
            "Use GitHub's official installation path for this operating system and its documented Node.js prerequisites."
        }
    }
}

#[cfg(test)]
#[path = "install_channels/tests.rs"]
mod tests;
