use super::super::super::ApplicationBoundary;
use serde_json::json;
use std::fs;

pub(super) fn configure(
    boundary: &ApplicationBoundary,
    argv: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    fs::create_dir_all(boundary.scratch.path.join(".agents"))?;
    let servers = boundary
        .server_args()?
        .map(|args| {
            json!({
                "command": args[0],
                "args": &args[1..],
                "cwd": boundary.working_directory(),
            })
        })
        .map_or_else(
            || json!({}),
            |server| json!({ (boundary.mcp_server_name()): server }),
        );
    boundary
        .scratch
        .write_json(".agents/mcp_config.json", &json!({ "mcpServers": servers }))?;
    let allowed = boundary
        .server_config
        .as_ref()
        .map(|_| format!("mcp({}/*)", boundary.mcp_server_name()));
    let mut allow = Vec::new();
    if let Some(rule) = allowed {
        allow.push(rule);
    }
    let home = boundary.scratch.path.join("cli-home");
    fs::create_dir(&home)?;
    boundary.scratch.write_json(
        "cli-home/.gemini/antigravity-cli/settings.json",
        &json!({
            "toolPermission": "strict",
            "allowNonWorkspaceAccess": false,
            "enableTerminalSandbox": true,
            "permissions": {
                "allow": allow,
                "deny": [
                    "read_file(*)",
                    "write_file(*)",
                    "command(*)",
                    "unsandboxed(*)",
                    "read_url(*)",
                    "execute_url(*)",
                ],
            },
        }),
    )?;
    let home = home.to_string_lossy().into_owned();
    env.extend([
        ("HOME".into(), home.clone()),
        ("XDG_CONFIG_HOME".into(), format!("{home}/.config")),
        ("XDG_DATA_HOME".into(), format!("{home}/.local/share")),
        ("XDG_CACHE_HOME".into(), format!("{home}/.cache")),
    ]);
    argv.push("--sandbox".into());
    Ok(())
}
