use super::super::{CliProvider, configure};
use super::{read_json, test_boundary};
use crate::harness::ToolAccess;
use serde_json::Value;

#[test]
fn codex_receives_only_the_private_application_mcp_server() {
    let boundary = test_boundary(ToolAccess::BoundedImplementation);
    let mut args = Vec::new();
    configure(&boundary, CliProvider::Codex, &mut args, &mut Vec::new()).unwrap();
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"])
    );
    assert!(
        args.windows(2)
            .any(|pair| { pair == ["--cd", boundary.working_directory().to_str().unwrap()] })
    );
    assert!(args.iter().any(|arg| arg == "--ignore-user-config"));
    assert!(args.iter().any(|arg| arg == "--ignore-rules"));
    assert!(args.iter().any(|arg| arg == "--skip-git-repo-check"));
    for disabled in [
        "shell_tool",
        "unified_exec",
        "workspace_dependencies",
        "memories",
        "view_image",
        "plugins",
    ] {
        assert!(args.windows(2).any(|pair| pair == ["--disable", disabled]));
    }
    assert!(
        args.iter()
            .any(|arg| arg.contains(boundary.server_name.as_str()))
    );
    assert!(args.iter().any(|arg| arg.contains("--internal-mcp-server")));
}

#[test]
fn private_boundary_files_are_created_with_restricted_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let boundary = test_boundary(ToolAccess::BoundedImplementation);
    let config = boundary
        .scratch
        .write_json("private.json", &serde_json::json!({}))
        .unwrap();
    assert_eq!(
        boundary
            .scratch
            .path
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        config.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn no_access_mode_disables_native_repository_tools_for_every_supported_provider() {
    let boundary = test_boundary(ToolAccess::None);
    for provider in [
        CliProvider::Codex,
        CliProvider::Claude,
        CliProvider::Antigravity,
        CliProvider::OpenCode,
        CliProvider::Copilot,
    ] {
        let mut args = Vec::new();
        let mut env = Vec::new();
        configure(&boundary, provider, &mut args, &mut env).unwrap();
        assert!(boundary.mcp_tool_names().is_empty());
        match provider {
            CliProvider::Codex => assert!(
                args.windows(2)
                    .any(|pair| pair == ["--disable", "shell_tool"])
            ),
            CliProvider::Claude => assert!(args.windows(2).any(|pair| pair == ["--tools", ""])),
            CliProvider::Antigravity => assert!(args.iter().any(|arg| arg == "--sandbox")),
            CliProvider::OpenCode => {
                assert!(env.iter().any(|(key, _)| key == "OPENCODE_CONFIG_CONTENT"))
            }
            CliProvider::Copilot => assert!(
                args.iter().any(|arg| arg == "--available-tools=")
                    || args
                        .windows(2)
                        .any(|pair| pair == ["--available-tools", ""])
            ),
        }
    }
    let claude: Value = read_json(&boundary.scratch.path.join("claude-mcp.json"));
    assert!(claude["mcpServers"].as_object().unwrap().is_empty());
    let antigravity: Value = read_json(&boundary.scratch.path.join(".agents/mcp_config.json"));
    assert!(antigravity["mcpServers"].as_object().unwrap().is_empty());
    let opencode: Value = read_json(&boundary.scratch.path.join("opencode.json"));
    assert!(opencode["mcp"].as_object().unwrap().is_empty());
    assert_eq!(opencode["tools"]["*"], false);
    assert_eq!(
        opencode["agent"][boundary.server_name.as_str()]["tools"]["*"],
        false
    );
    let copilot: Value = read_json(&boundary.scratch.path.join("copilot-mcp.json"));
    assert!(copilot["mcpServers"].as_object().unwrap().is_empty());
}

#[test]
fn every_host_provider_drops_node_startup_options() {
    for provider in [
        CliProvider::Codex,
        CliProvider::Claude,
        CliProvider::Antigravity,
        CliProvider::OpenCode,
        CliProvider::Copilot,
    ] {
        assert!(
            provider
                .excluded_child_environment()
                .contains(&"NODE_OPTIONS"),
            "{provider:?}"
        );
    }
    assert!(
        CliProvider::Claude
            .excluded_child_environment()
            .contains(&"CLAUDE_CODE_SHELL_PREFIX")
    );
}
