use super::super::{CliProvider, configure};
use super::{read_json, test_boundary};
use crate::harness::ToolAccess;
use serde_json::Value;

#[test]
fn claude_is_restricted_to_exact_application_mcp_tools() {
    let boundary = test_boundary(ToolAccess::BoundedImplementation);
    let mut args = Vec::new();
    configure(&boundary, CliProvider::Claude, &mut args, &mut Vec::new()).unwrap();
    assert!(args.windows(2).any(|pair| pair == ["--tools", ""]));
    assert!(args.iter().any(|arg| arg == "--restricted"));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--permission-mode", "dontAsk"])
    );
    assert!(args.iter().any(|arg| arg == "--strict-mcp-config"));
    let allowed = args.iter().position(|arg| arg == "--allowedTools").unwrap();
    assert_eq!(
        args[allowed + 1],
        ["koolade_bash", "koolade_resource", "koolade_dependency"]
            .into_iter()
            .map(|tool| format!("mcp__{}__{tool}", boundary.server_name))
            .collect::<Vec<_>>()
            .join(",")
    );
}

#[test]
fn antigravity_uses_private_home_and_only_app_mcp_permissions() {
    let boundary = test_boundary(ToolAccess::BoundedImplementation);
    let mut args = Vec::new();
    let mut env = Vec::new();
    configure(&boundary, CliProvider::Antigravity, &mut args, &mut env).unwrap();
    assert!(args.iter().any(|arg| arg == "--sandbox"));
    assert!(
        !args
            .iter()
            .any(|arg| arg == "--extensions" || arg == "--allowed-mcp-server-names")
    );
    let settings: Value = read_json(
        &boundary
            .scratch
            .path
            .join("cli-home/.gemini/antigravity-cli/settings.json"),
    );
    assert_eq!(
        settings["permissions"]["allow"][0],
        format!("mcp({}/*)", boundary.server_name)
    );
    for rule in [
        "read_file(*)",
        "write_file(*)",
        "command(*)",
        "unsandboxed(*)",
        "read_url(*)",
        "execute_url(*)",
    ] {
        assert!(
            settings["permissions"]["deny"]
                .as_array()
                .unwrap()
                .contains(&Value::String(rule.into()))
        );
    }
    assert!(
        env.contains(&(
            "HOME".into(),
            boundary
                .scratch
                .path
                .join("cli-home")
                .to_string_lossy()
                .into_owned(),
        ))
    );
    for (key, relative) in [
        ("XDG_CONFIG_HOME", ".config"),
        ("XDG_DATA_HOME", ".local/share"),
        ("XDG_CACHE_HOME", ".cache"),
    ] {
        assert!(
            env.contains(&(
                key.into(),
                boundary
                    .scratch
                    .path
                    .join("cli-home")
                    .join(relative)
                    .to_string_lossy()
                    .into_owned(),
            ))
        );
    }
    let mcp: Value = read_json(&boundary.scratch.path.join(".agents/mcp_config.json"));
    assert!(
        mcp["mcpServers"]
            .get(boundary.server_name.as_str())
            .is_some()
    );
    assert_eq!(mcp["mcpServers"].as_object().unwrap().len(), 1);
}

#[test]
fn opencode_denies_native_tools_and_allows_only_the_private_server() {
    let boundary = test_boundary(ToolAccess::BoundedImplementation);
    let mut args = vec!["--dir".into(), "/old/workspace".into()];
    let mut env = Vec::new();
    configure(&boundary, CliProvider::OpenCode, &mut args, &mut env).unwrap();
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--dir", boundary.working_directory().to_str().unwrap()])
    );
    let config: Value = read_json(&boundary.scratch.path.join("opencode.json"));
    assert_eq!(config["permission"]["*"], "deny");
    assert_eq!(config["tools"]["*"], false);
    assert_eq!(
        config["permission"][format!("{}_*", boundary.server_name)],
        "allow"
    );
    assert!(config["mcp"].get(boundary.server_name.as_str()).is_some());
    assert!(env.iter().any(|(key, _)| key == "OPENCODE_CONFIG_CONTENT"));
    assert!(env.iter().any(|(key, _)| key == "XDG_CONFIG_HOME"));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--agent", boundary.server_name.as_str()])
    );
    assert_eq!(
        config["agent"][boundary.server_name.as_str()]["tools"]["*"],
        false
    );
}

#[test]
fn copilot_has_only_the_private_mcp_tools_in_its_available_tool_list() {
    let boundary = test_boundary(ToolAccess::BoundedImplementation);
    let mut args = vec!["--available-tools=read,edit".into()];
    let mut env = Vec::new();
    configure(&boundary, CliProvider::Copilot, &mut args, &mut env).unwrap();
    let available = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--available-tools="))
        .unwrap();
    assert_eq!(
        available,
        ["koolade_bash", "koolade_resource", "koolade_dependency"]
            .into_iter()
            .map(|tool| format!("{}-{tool}", boundary.server_name))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(args.iter().any(|arg| arg == "--disable-builtin-mcps"));
    assert!(args.iter().any(|arg| arg == "--no-custom-instructions"));
    assert!(args.iter().any(|arg| arg == "--no-remote"));
    assert!(args.iter().any(|arg| arg == "--no-remote-export"));
    assert!(
        args.iter()
            .any(|arg| arg == &format!("--allow-tool={}", boundary.server_name))
    );
    let config: Value = read_json(&boundary.scratch.path.join("copilot-mcp.json"));
    assert!(
        config["mcpServers"]
            .get(boundary.server_name.as_str())
            .is_some()
    );
    assert!(
        env.contains(&(
            "COPILOT_HOME".into(),
            boundary
                .scratch
                .path
                .join("copilot-home")
                .to_string_lossy()
                .into_owned(),
        ))
    );
    assert!(
        env.contains(&(
            "COPILOT_CACHE_HOME".into(),
            boundary
                .scratch
                .path
                .join("copilot-home/cache")
                .to_string_lossy()
                .into_owned(),
        ))
    );
}
