use super::{CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch};
use std::path::Path;

pub(super) fn validate_profile(launch: &ValidatedProviderLaunch) -> anyhow::Result<()> {
    #[cfg(test)]
    let explicit_uncontained_test = crate::harness::uncontained_provider_test_execution_enabled();
    #[cfg(not(test))]
    let explicit_uncontained_test = false;
    if !explicit_uncontained_test
        && let Some(reason) = crate::harness::execution_security::profile_block_reason(
            launch.evidence.provider.telemetry_id(),
            launch.profile,
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
    if launch.evidence.provider == super::CliProvider::Pi {
        validate_pi_tool_controls(launch)?;
        anyhow::ensure!(
            validate_pi_skill_controls(launch),
            "Pi launch may use only the application-curated worker skill in bounded implementation; other modes must have no explicit skill"
        );
    }
    if launch.evidence.provider == CliProvider::Claude {
        validate_claude_update_lock(launch)?;
    }
    Ok(())
}

fn validate_pi_tool_controls(launch: &ValidatedProviderLaunch) -> anyhow::Result<()> {
    let argv = &launch.argv;
    anyhow::ensure!(
        !argv.iter().any(|argument| {
            ["--tools=", "--extension="]
                .iter()
                .any(|flag| argument.starts_with(flag))
        }),
        "Pi launch must use the validated separate tool and extension arguments"
    );
    let no_tools = argv
        .iter()
        .filter(|arg| arg.as_str() == "--no-tools")
        .count();
    let tools = argv
        .windows(2)
        .filter(|pair| pair[0] == "--tools")
        .collect::<Vec<_>>();
    let extensions = argv
        .windows(2)
        .filter(|pair| pair[0] == "--extension")
        .collect::<Vec<_>>();
    let approved_tools: &[&str] = match launch.mode.tool_access() {
        crate::harness::ToolAccess::None => &[],
        crate::harness::ToolAccess::ReadOnly => &["koolade_bash"],
        crate::harness::ToolAccess::InvestigationBroker => &["koolade_investigate"],
        crate::harness::ToolAccess::BoundedImplementation => {
            &["koolade_bash", "koolade_resource", "koolade_dependency"]
        }
        crate::harness::ToolAccess::RuntimeQaScreenshots => &["koolade_runtime_qa_screenshot"],
    };
    // Missing broker configuration must reduce a normally tool-capable role
    // to a tool-free launch. That is fail-closed and remains a valid profile.
    let tool_free = no_tools == 1 && tools.is_empty() && extensions.is_empty();
    let brokered_tools = !approved_tools.is_empty()
        && no_tools == 0
        && tools.len() == 1
        && extensions.len() == 1
        && tools[0][1].split(',').collect::<Vec<_>>() == approved_tools
        && validate_app_extension(launch, &extensions[0][1]);
    anyhow::ensure!(
        argv.iter().any(|arg| arg == "--no-builtin-tools") && (tool_free || brokered_tools),
        "Pi launch must disable built-in tools and expose either no tools or one app-brokered tool extension"
    );
    Ok(())
}

fn validate_app_extension(launch: &ValidatedProviderLaunch, extension: &str) -> bool {
    let expected = launch.cwd.join("pi-koolade-mcp.ts");
    let path = Path::new(extension);
    if path != expected || !path.is_absolute() {
        return false;
    }
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    metadata.is_file()
        && !metadata.file_type().is_symlink()
        && std::fs::read(path).is_ok_and(|contents| {
            contents.as_slice() == include_bytes!("../../pi_mcp_extension.ts")
        })
}

fn validate_pi_skill_controls(launch: &ValidatedProviderLaunch) -> bool {
    if launch
        .argv
        .iter()
        .any(|argument| argument.starts_with("--skill="))
    {
        return false;
    }
    let flags = launch
        .argv
        .iter()
        .filter(|argument| argument.as_str() == "--skill")
        .count();
    if launch.mode != crate::harness::ExecutionMode::Implementation {
        return flags == 0;
    }
    // An unconfigured implementation broker is intentionally reduced to a
    // tool-free launch; it must not need an app skill just to start safely.
    if flags == 0 {
        return launch.argv.iter().any(|argument| argument == "--no-tools")
            && !launch
                .argv
                .iter()
                .any(|argument| argument == "--tools" || argument == "--extension");
    }
    if flags != 1 {
        return false;
    }
    let Some(skill) = launch
        .argv
        .windows(2)
        .find(|arguments| arguments[0] == "--skill")
        .map(|arguments| Path::new(&arguments[1]))
    else {
        return false;
    };
    let expected = launch.cwd.join("worker-skills/koolade-task/SKILL.md");
    skill == expected
        && skill.is_absolute()
        && real_directory(&launch.cwd)
        && real_directory(&launch.cwd.join("worker-skills"))
        && real_directory(&launch.cwd.join("worker-skills/koolade-task"))
        && real_file(skill)
        && std::fs::read(skill)
            .is_ok_and(|contents| contents.as_slice() == include_bytes!("../../pi_worker_skill.md"))
}

fn real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn real_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
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
        launch.mode,
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
