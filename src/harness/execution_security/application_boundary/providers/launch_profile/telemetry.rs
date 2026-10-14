use super::{ValidatedProviderLaunch, parse_version};

pub(super) fn for_launch(
    launch: &ValidatedProviderLaunch,
    prompt_role_version: Option<&str>,
) -> crate::harness::CleanLaunchTelemetry {
    let provider = launch.evidence.provider;
    let requirements = provider.profile_requirements();
    let version = parse_version(&launch.evidence.version)
        .map(|(major, minor, patch)| format!("{major}.{minor}.{patch}"))
        .unwrap_or_default();
    let identity = format!(
        "{}|{}|{}|{}",
        provider.name(),
        version,
        requirements.capabilities.join("|"),
        requirements.launch_tokens.join("|")
    );
    use sha2::Digest;
    crate::harness::CleanLaunchTelemetry {
        provider: provider.name().to_owned(),
        cli_version: version,
        prompt_role_version: prompt_role_version.map(str::to_owned),
        profile_hash: format!("{:x}", sha2::Sha256::digest(identity.as_bytes())),
        verified_capabilities: launch.evidence.capabilities.clone(),
    }
}
