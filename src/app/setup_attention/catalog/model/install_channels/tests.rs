use super::*;

fn classify(
    provider: ProviderIdV1,
    host: HostKind,
    distribution: Option<LinuxDistributionV1>,
    version: Option<(u16, u16)>,
    architecture: ImageArchitectureV1,
    channel: ProviderInstallChannelV1,
) -> ProviderInstallClassificationV1 {
    classify_provider_install_channel(
        provider,
        host,
        distribution,
        version.map(|(major, minor)| LinuxDistributionVersionV1 { major, minor }),
        architecture,
        channel,
    )
}

#[test]
fn only_exact_claude_ubuntu_signed_binary_is_app_owned() {
    assert_eq!(
        classify(
            ProviderIdV1::Claude,
            HostKind::Linux,
            Some(LinuxDistributionV1::Ubuntu),
            Some((24, 4)),
            ImageArchitectureV1::Amd64,
            ProviderInstallChannelV1::ClaudeSignedBinary,
        ),
        ProviderInstallClassificationV1::AppOwnedReviewedRecipe
    );
    for (provider, distribution, version, architecture) in [
        (
            ProviderIdV1::Claude,
            Some(LinuxDistributionV1::Ubuntu),
            Some((22, 4)),
            ImageArchitectureV1::Amd64,
        ),
        (
            ProviderIdV1::Claude,
            Some(LinuxDistributionV1::Ubuntu),
            Some((24, 4)),
            ImageArchitectureV1::Arm64,
        ),
        (
            ProviderIdV1::Claude,
            Some(LinuxDistributionV1::Debian),
            Some((24, 4)),
            ImageArchitectureV1::Amd64,
        ),
        (
            ProviderIdV1::Codex,
            Some(LinuxDistributionV1::Ubuntu),
            Some((24, 4)),
            ImageArchitectureV1::Amd64,
        ),
    ] {
        assert_eq!(
            classify(
                provider,
                HostKind::Linux,
                distribution,
                version,
                architecture,
                ProviderInstallChannelV1::ClaudeSignedBinary,
            ),
            ProviderInstallClassificationV1::UnsupportedCombination
        );
    }
    assert_eq!(
        classify(
            ProviderIdV1::Claude,
            HostKind::Linux,
            Some(LinuxDistributionV1::Ubuntu),
            None,
            ImageArchitectureV1::Amd64,
            ProviderInstallChannelV1::ClaudeSignedBinary,
        ),
        ProviderInstallClassificationV1::UnsupportedCombination
    );
    assert_eq!(
        classify(
            ProviderIdV1::Claude,
            HostKind::MacOs,
            None,
            None,
            ImageArchitectureV1::Amd64,
            ProviderInstallChannelV1::ClaudeSignedBinary,
        ),
        ProviderInstallClassificationV1::UnsupportedCombination
    );
}

#[test]
fn linux_package_channel_families_are_vendor_guide_only() {
    for (distribution, channel) in [
        (
            LinuxDistributionV1::Ubuntu,
            ProviderInstallChannelV1::AptLike,
        ),
        (
            LinuxDistributionV1::Debian,
            ProviderInstallChannelV1::AptLike,
        ),
        (
            LinuxDistributionV1::PopOs,
            ProviderInstallChannelV1::AptLike,
        ),
        (
            LinuxDistributionV1::Fedora,
            ProviderInstallChannelV1::RpmLike,
        ),
        (LinuxDistributionV1::Suse, ProviderInstallChannelV1::RpmLike),
        (
            LinuxDistributionV1::Arch,
            ProviderInstallChannelV1::ArchLike,
        ),
        (
            LinuxDistributionV1::Alpine,
            ProviderInstallChannelV1::AlpineApk,
        ),
    ] {
        assert_eq!(
            classify(
                ProviderIdV1::Claude,
                HostKind::Linux,
                Some(distribution),
                None,
                ImageArchitectureV1::Amd64,
                channel,
            ),
            ProviderInstallClassificationV1::OfficialVendorGuideOnly,
            "unexpected classification for {distribution:?} via {channel:?}"
        );
    }
}

#[test]
fn macos_and_windows_channels_are_vendor_guide_only() {
    for (host, channel) in [
        (HostKind::MacOs, ProviderInstallChannelV1::MacHomebrew),
        (HostKind::MacOs, ProviderInstallChannelV1::MacPackage),
        (HostKind::Windows, ProviderInstallChannelV1::WindowsWinget),
        (HostKind::Windows, ProviderInstallChannelV1::WindowsMsi),
    ] {
        assert_eq!(
            classify(
                ProviderIdV1::Claude,
                host,
                None,
                None,
                ImageArchitectureV1::Amd64,
                channel,
            ),
            ProviderInstallClassificationV1::OfficialVendorGuideOnly,
            "unexpected classification for {host:?} via {channel:?}"
        );
    }
}

#[test]
fn unknown_distro_channels_stay_manual_and_mismatches_are_unsupported() {
    for distribution in [None, Some(LinuxDistributionV1::Other)] {
        assert_eq!(
            classify(
                ProviderIdV1::Claude,
                HostKind::Linux,
                distribution,
                None,
                ImageArchitectureV1::Unknown,
                ProviderInstallChannelV1::AptLike,
            ),
            ProviderInstallClassificationV1::OfficialVendorGuideOnly
        );
    }
    for (host, distribution, channel) in [
        (
            HostKind::Linux,
            Some(LinuxDistributionV1::Fedora),
            ProviderInstallChannelV1::AptLike,
        ),
        (
            HostKind::Linux,
            Some(LinuxDistributionV1::Debian),
            ProviderInstallChannelV1::RpmLike,
        ),
        (
            HostKind::Windows,
            None,
            ProviderInstallChannelV1::MacHomebrew,
        ),
        (
            HostKind::Linux,
            None,
            ProviderInstallChannelV1::WindowsWinget,
        ),
        (
            HostKind::Unsupported,
            None,
            ProviderInstallChannelV1::Unknown,
        ),
    ] {
        assert_eq!(
            classify(
                ProviderIdV1::Claude,
                host,
                distribution,
                None,
                ImageArchitectureV1::Unknown,
                channel,
            ),
            ProviderInstallClassificationV1::UnsupportedCombination,
            "unexpected classification for {host:?}, {distribution:?}, {channel:?}"
        );
    }
    assert_eq!(
        classify(
            ProviderIdV1::Claude,
            HostKind::Linux,
            None,
            None,
            ImageArchitectureV1::Unknown,
            ProviderInstallChannelV1::Unknown,
        ),
        ProviderInstallClassificationV1::OfficialVendorGuideOnly
    );
}

#[test]
fn displayed_channel_guidance_is_manual_and_contains_no_commands() {
    for (provider, host) in [
        (ProviderIdV1::Claude, HostKind::Linux),
        (ProviderIdV1::Codex, HostKind::MacOs),
        (ProviderIdV1::Copilot, HostKind::Windows),
    ] {
        let note = provider.install_channel_note(host);
        assert!(note.contains("manual-only"));
        assert!(note.contains("does not infer"));
        assert!(note.contains("guide documents"));
    }
}

#[test]
fn model_and_install_links_use_current_provider_guides() {
    assert_eq!(
        ProviderIdV1::OpenCode.installation_guide(),
        "https://opencode.ai/v2/docs/"
    );
    assert_eq!(
        ProviderIdV1::Copilot.model_guide(),
        "https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference"
    );
    assert_eq!(
        ProviderIdV1::Antigravity.installation_guide(),
        "https://antigravity.google/docs/getting-started"
    );
    assert_eq!(
        ProviderIdV1::Antigravity.model_guide(),
        "https://antigravity.google/docs/models/"
    );
}
