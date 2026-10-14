use super::{BoundaryAccess, ServerConfig, resource};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn list(config: &ServerConfig) -> Vec<Value> {
    let mut tools = vec![json!({
        "name": "koolade_bash",
        "description": match config.access {
            BoundaryAccess::ReadOnly => "Run a read-only shell command in the application-selected workspace inside Kool.ad/e's Docker planning session.",
            BoundaryAccess::Implementation => "Run a shell command in the assigned task clone inside Kool.ad/e's Docker implementation session. Network and host credentials are unavailable.",
        },
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to run inside the Docker execution session." },
                "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 1200 },
            },
            "required": ["command"],
            "additionalProperties": false,
        }
    })];
    if config.access == BoundaryAccess::Implementation {
        tools.extend([
            json!({
                "name": "koolade_resource",
                "description": "Ask Kool.ad/e to retrieve one public non-package HTTPS resource. Package requests must use koolade_dependency.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Exact HTTPS URL." },
                        "purpose": { "type": "string", "description": "Why this task needs the resource." },
                    },
                    "required": ["url", "purpose"],
                    "additionalProperties": false,
                }
            }),
            json!({
                "name": "koolade_dependency",
                "description": "Submit a package need to Kool.ad/e Man.ager. The application owns task identity and authorization; this tool cannot grant access or change sandbox permissions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ecosystem": { "type": "string", "enum": ["npm", "pnpm", "yarn", "cargo", "nuget", "pip", "uv", "poetry", "system", "other"] },
                        "package": { "type": "string" },
                        "version": { "type": "string" },
                        "source": { "type": "string" },
                        "command": { "type": "string", "description": "Exact package-manager command." },
                        "reason": { "type": "string", "description": "Task-specific reason for needing the package." },
                        "kind": { "type": "string", "enum": ["existing_restore", "new_project_dependency", "development_dependency", "system_tool"] },
                    },
                    "required": ["ecosystem", "command", "reason", "kind"],
                    "additionalProperties": false,
                }
            }),
        ]);
    }
    tools
}

pub(super) fn call(
    config_path: &Path,
    config: &ServerConfig,
    name: &str,
    arguments: &Value,
) -> Value {
    let result = match name {
        "koolade_bash" => bash(config_path, config, arguments),
        "koolade_resource" if config.access == BoundaryAccess::Implementation => {
            resource::fetch(config, arguments)
        }
        "koolade_dependency" if config.access == BoundaryAccess::Implementation => {
            resource::dependency(config_path, config, arguments)
        }
        _ => Err(format!(
            "Tool {name} is not available in this Kool.ad/e execution mode"
        )),
    };
    let (text, is_error) = result.unwrap_or_else(|error| (error, true));
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn bash(
    config_path: &Path,
    config: &ServerConfig,
    arguments: &Value,
) -> Result<(String, bool), String> {
    let Some(command) = arguments["command"]
        .as_str()
        .filter(|value| !value.trim().is_empty() && value.len() <= 65_536)
    else {
        return Err("A non-empty shell command up to 64 KiB is required".into());
    };
    let timeout = arguments["timeout_seconds"]
        .as_u64()
        .unwrap_or(300)
        .clamp(1, 1200);
    let binding = super::super::session_bridge::SessionBinding {
        session_id: config.session_id.clone(),
        capability_id: config.capability_id.clone(),
        target_id: config.target_id.clone().unwrap_or_else(|| "local".into()),
        target_arch: config.target_arch.clone(),
        task_id: config.task_id.clone(),
        workspace_id: config.workspace_id.clone(),
        access: match config.access {
            BoundaryAccess::ReadOnly => "read_only".into(),
            BoundaryAccess::Implementation => "implementation".into(),
        },
    };
    let output = super::super::session_bridge::call(config_path, &binding, command, timeout)
        .map_err(|error| format!("Docker execution session is unavailable: {error:#}"))?;
    let mut text = output.stdout;
    if !output.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&output.stderr);
    }
    if let Some(code) = output.exit_code {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("Exit code: {code}"));
    }
    Ok((text, output.is_error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_mode_only_exposes_the_sandboxed_shell_tool() {
        let config = config(BoundaryAccess::ReadOnly);
        let listed = list(&config);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["name"], "koolade_bash");
        let unavailable = call(
            Path::new("/tmp/mcp/server.json"),
            &config,
            "koolade_resource",
            &Value::Null,
        );
        assert_eq!(unavailable["isError"], true);
    }

    #[test]
    fn implementation_tools_are_exactly_the_shared_policy_surface() {
        let config = config(BoundaryAccess::Implementation);
        let listed = list(&config);
        let names = listed
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["koolade_bash", "koolade_resource", "koolade_dependency"]
        );
    }

    fn config(access: BoundaryAccess) -> ServerConfig {
        ServerConfig {
            protocol_version: 2,
            access,
            session_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            workspace_id: format!(
                "00000000-0000-4000-8000-000000000003:00000000-0000-4000-8000-000000000002:{}",
                "a".repeat(40)
            ),
            capability_id: "00000000-0000-4000-8000-000000000004".into(),
            target_id: Some("local".into()),
            target_arch: Some("amd64".into()),
            resource_socket: None,
            resource_cache: None,
        }
    }
}
