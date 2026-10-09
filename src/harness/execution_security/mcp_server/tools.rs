use super::{BoundaryAccess, ServerConfig, resource, sandbox};
use serde_json::{Value, json};

pub(super) fn list(config: &ServerConfig) -> Vec<Value> {
    let mut tools = vec![json!({
        "name": "koolade_bash",
        "description": match config.access {
            BoundaryAccess::ReadOnly => "Run a read-only shell command in the assigned project inside Kool.ad/e's Bubblewrap planning sandbox.",
            BoundaryAccess::Implementation => "Run a shell command in the assigned task clone inside Kool.ad/e's Bubblewrap implementation sandbox. Network and host credentials are unavailable.",
        },
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to run inside the Bubblewrap sandbox." },
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

pub(super) fn call(config: &ServerConfig, name: &str, arguments: &Value) -> Value {
    let result = match name {
        "koolade_bash" => bash(config, arguments),
        "koolade_resource" if config.access == BoundaryAccess::Implementation => {
            resource::fetch(config, arguments)
        }
        "koolade_dependency" if config.access == BoundaryAccess::Implementation => {
            resource::dependency(config, arguments)
        }
        _ => Err(format!(
            "Tool {name} is not available in this Kool.ad/e execution mode"
        )),
    };
    let (text, is_error) = result.unwrap_or_else(|error| (error, true));
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn bash(config: &ServerConfig, arguments: &Value) -> Result<(String, bool), String> {
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
    let output = sandbox::run(config, command, timeout, false, None)
        .map_err(|error| format!("Bubblewrap command could not start: {error:#}"))?;
    Ok((output.text, output.is_error))
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
        let unavailable = call(&config, "koolade_resource", &Value::Null);
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
            access,
            bwrap: "/usr/bin/bwrap".into(),
            root: "/tmp/project".into(),
            args: Vec::new(),
            resource_socket: None,
            resource_cache: None,
        }
    }
}
