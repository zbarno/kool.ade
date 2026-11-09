use super::super::ValidatedProviderLaunch;
use std::path::Path;

pub(super) fn validate_pi_tool_controls(launch: &ValidatedProviderLaunch) -> anyhow::Result<()> {
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
    let argv_tools = tools
        .first()
        .map(|pair| pair[1].split(',').collect::<Vec<_>>());
    let base_tools_match = argv_tools.as_deref() == Some(approved_tools);
    let registered_tools_match = registered_pi_tools_match(launch, argv_tools.as_deref());
    let computer_use_tools_match = launch.mode == crate::harness::ExecutionMode::Implementation
        && !approved_tools.is_empty()
        && registered_computer_use_tools(launch, argv_tools.as_deref());
    let brokered_tools = !approved_tools.is_empty()
        && no_tools == 0
        && tools.len() == 1
        && extensions.len() == 1
        && registered_tools_match
        && (base_tools_match || computer_use_tools_match)
        && validate_app_extension(launch, &extensions[0][1]);
    let no_builtin_tools = argv.iter().any(|arg| arg == "--no-builtin-tools");
    anyhow::ensure!(
        no_builtin_tools && (tool_free || brokered_tools),
        "Pi launch must disable built-in tools and expose either no tools or one app-brokered tool extension (mode={:?}, --no-tools count={no_tools}, --tools values={:?}, --extension count={}, --no-builtin-tools={no_builtin_tools})",
        launch.mode,
        tools
            .iter()
            .map(|pair| pair[1].as_str())
            .collect::<Vec<_>>(),
        extensions.len(),
    );
    Ok(())
}

fn registered_pi_tools_match(
    launch: &ValidatedProviderLaunch,
    argv_tools: Option<&[&str]>,
) -> bool {
    let tool_config = launch
        .env
        .iter()
        .filter(|(name, _)| name == "KOOLADE_MCP_TOOLS")
        .collect::<Vec<_>>();
    if tool_config.is_empty() {
        // Older hand-built profile fixtures omit the application extension
        // environment; production Pi launches always bind this registration.
        return true;
    }
    if tool_config.len() != 1 {
        return false;
    }
    let Ok(registered) = serde_json::from_str::<Vec<String>>(&tool_config[0].1) else {
        return false;
    };
    argv_tools
        == Some(
            registered
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )
}

fn registered_computer_use_tools(
    launch: &ValidatedProviderLaunch,
    argv_tools: Option<&[&str]>,
) -> bool {
    let tool_config = launch
        .env
        .iter()
        .filter(|(name, _)| name == "KOOLADE_MCP_TOOLS")
        .collect::<Vec<_>>();
    if tool_config.len() != 1 {
        return false;
    }
    let Ok(registered) = serde_json::from_str::<Vec<String>>(&tool_config[0].1) else {
        return false;
    };
    let mut expected = ["koolade_bash", "koolade_resource", "koolade_dependency"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    expected.extend(crate::harness::execution_security::computer_use_bridge::tool_names());
    argv_tools
        == Some(
            expected
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )
        && registered == expected
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
            contents.as_slice() == include_bytes!("../../../pi_mcp_extension.ts")
        })
}
