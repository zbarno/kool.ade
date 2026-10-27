use super::{CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch};
use std::path::Path;

pub(super) fn validate_profile(launch: &ValidatedProviderLaunch) -> anyhow::Result<()> {
    #[cfg(test)]
    let explicit_uncontained_test = crate::harness::uncontained_provider_test_execution_enabled();
    #[cfg(not(test))]
    let explicit_uncontained_test = false;
    if !explicit_uncontained_test
        && let Some(reason) = crate::harness::execution_security::task_execution_block_reason(
            launch.evidence.provider.telemetry_id(),
        )
    {
        anyhow::bail!("{reason}");
    }
    launch.evidence.validate()?;
    let executable = launch
        .argv
        .first()
        .ok_or_else(|| anyhow::anyhow!("provider argv is empty"))?;
    anyhow::ensure!(
        Path::new(executable) == launch.evidence.executable,
        "provider launch executable does not match its validated probe"
    );
    let requirements = launch.evidence.provider.profile_requirements();
    let missing = requirements
        .launch_tokens
        .iter()
        .filter(|token| {
            if token.ends_with('=') {
                !launch
                    .argv
                    .iter()
                    .any(|argument| argument.starts_with(**token))
            } else {
                !launch.argv.iter().any(|argument| argument == **token)
            }
        })
        .copied()
        .collect::<Vec<_>>();
    let missing_env = requirements
        .environment_keys
        .iter()
        .filter(|key| {
            !launch
                .env
                .iter()
                .any(|(name, value)| name == **key && !value.is_empty())
        })
        .copied()
        .collect::<Vec<_>>();
    anyhow::ensure!(
        missing.is_empty() && missing_env.is_empty(),
        "{} launch profile is missing required arguments [{}] or environment entries [{}]",
        launch.evidence.provider.name(),
        missing.join(", "),
        missing_env.join(", "),
    );
    if launch.evidence.provider == CliProvider::Claude {
        validate_claude_update_lock(launch)?;
    }
    Ok(())
}

fn validate_claude_update_lock(launch: &ValidatedProviderLaunch) -> anyhow::Result<()> {
    let settings = launch
        .argv
        .windows(2)
        .filter(|pair| pair[0] == "--settings")
        .collect::<Vec<_>>();
    anyhow::ensure!(
        settings.len() == 1
            && settings[0][1] == crate::harness::claude_harness::UPDATE_LOCK_SETTINGS,
        "Claude launch must bind exactly one fixed update-lock settings value"
    );
    let update_locks = launch
        .env
        .iter()
        .filter(|(name, _)| name == "DISABLE_UPDATES")
        .collect::<Vec<_>>();
    anyhow::ensure!(
        update_locks.len() == 1 && update_locks[0].1 == "1",
        "Claude launch must bind the fixed update-lock environment value"
    );
    Ok(())
}

pub(super) fn fingerprint(launch: &ValidatedProviderLaunch) -> anyhow::Result<String> {
    let evidence: &ProfileProbeEvidence = &launch.evidence;
    let bytes = serde_json::to_vec(&(
        evidence.provider.name(),
        &evidence.executable,
        &evidence.version,
        &evidence.capabilities,
        &launch.argv,
        &launch.cwd,
        &launch.env,
    ))?;
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}
