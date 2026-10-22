use super::super::super::{ApplicationBoundary, CliProvider};
use serde_json::Value;
use std::{fs, path::Path};

pub(super) fn assert_server_registration(
    provider: CliProvider,
    boundary: &ApplicationBoundary,
    argv: &[String],
    env: &[(String, String)],
) -> Option<Vec<String>> {
    let expected = boundary.server_args().unwrap();
    let server_name = boundary.mcp_server_name();
    let registered = match provider {
        CliProvider::Codex => codex_registration(argv, server_name, expected.as_deref()),
        CliProvider::Claude => {
            let path = arg_after(argv, "--mcp-config");
            let config = read_json(Path::new(path));
            json_registration(&config["mcpServers"], server_name, expected.as_deref())
        }
        CliProvider::Pi => pi_registration(boundary, argv, env, expected.as_deref()),
        CliProvider::Antigravity => {
            let config = read_json(&boundary.working_directory().join(".agents/mcp_config.json"));
            json_registration(&config["mcpServers"], server_name, expected.as_deref())
        }
        CliProvider::OpenCode => {
            let config: Value =
                serde_json::from_str(env_value(env, "OPENCODE_CONFIG_CONTENT")).unwrap();
            json_registration(&config["mcp"], server_name, expected.as_deref())
        }
        CliProvider::Copilot => {
            let path = argv
                .iter()
                .find_map(|arg| arg.strip_prefix("--additional-mcp-config=@"))
                .expect("Copilot should receive its isolated MCP config path");
            let config = read_json(Path::new(path));
            json_registration(&config["mcpServers"], server_name, expected.as_deref())
        }
    };
    assert_eq!(registered, expected, "{provider:?} registration changed");
    registered
}

fn codex_registration(
    argv: &[String],
    server_name: &str,
    expected: Option<&[String]>,
) -> Option<Vec<String>> {
    let settings = argv
        .windows(2)
        .filter(|pair| pair[0] == "--config")
        .map(|pair| pair[1].as_str())
        .collect::<Vec<_>>();
    if expected.is_none() {
        assert!(
            settings.contains(&"mcp_servers={}"),
            "Codex must explicitly disable MCP when the role has no tool access"
        );
        assert!(
            settings
                .iter()
                .all(|setting| !setting.starts_with("mcp_servers.")),
            "Codex must not register another MCP server for a role without tools"
        );
        return None;
    }

    let settings = settings.join("\n");
    let settings = settings.parse::<toml_edit::DocumentMut>().unwrap();
    let servers = settings.get("mcp_servers").unwrap().as_table().unwrap();
    let server_names = servers.iter().map(|(name, _)| name).collect::<Vec<_>>();
    assert_eq!(server_names, vec![server_name]);
    let server = servers
        .get(server_name)
        .expect("Codex MCP config omitted the app server")
        .as_table()
        .unwrap();
    let command = server
        .get("command")
        .and_then(toml_edit::Item::as_str)
        .expect("Codex MCP command must be a string");
    let args = server
        .get("args")
        .and_then(toml_edit::Item::as_array)
        .expect("Codex MCP arguments must be an array");
    let mut registered = vec![command.to_owned()];
    registered.extend(args.iter().map(|arg| {
        arg.as_str()
            .expect("Codex MCP argument must be a string")
            .to_owned()
    }));
    assert_eq!(registered, expected.unwrap());
    assert_eq!(
        server.get("enabled").and_then(toml_edit::Item::as_bool),
        Some(true)
    );
    assert_eq!(
        server.get("required").and_then(toml_edit::Item::as_bool),
        Some(true)
    );
    Some(registered)
}

fn json_registration(
    servers: &Value,
    server_name: &str,
    expected: Option<&[String]>,
) -> Option<Vec<String>> {
    let servers = servers
        .as_object()
        .expect("MCP server list must be an object");
    let mut server_names = servers.keys().map(String::as_str).collect::<Vec<_>>();
    server_names.sort_unstable();
    let expected_names = expected.map(|_| vec![server_name]).unwrap_or_default();
    assert_eq!(
        server_names, expected_names,
        "unexpected MCP server registration"
    );

    let expected = expected?;
    let server = servers.get(server_name).unwrap();
    let registered = match server.get("command") {
        Some(Value::String(command)) => {
            let args = server
                .get("args")
                .and_then(Value::as_array)
                .expect("string MCP command must have an argument array");
            std::iter::once(command.clone())
                .chain(args.iter().map(|arg| {
                    arg.as_str()
                        .expect("MCP command argument must be a string")
                        .to_owned()
                }))
                .collect::<Vec<_>>()
        }
        Some(Value::Array(command)) => {
            assert!(
                !server.as_object().unwrap().contains_key("args"),
                "array MCP command must not have a second argument array"
            );
            command
                .iter()
                .map(|arg| {
                    arg.as_str()
                        .expect("MCP command argument must be a string")
                        .to_owned()
                })
                .collect()
        }
        _ => panic!("MCP command must be a string or an argument array"),
    };
    assert!(!registered.is_empty(), "MCP command must not be empty");
    assert_eq!(registered, expected);
    Some(registered)
}

fn pi_registration(
    boundary: &ApplicationBoundary,
    argv: &[String],
    env: &[(String, String)],
    expected: Option<&[String]>,
) -> Option<Vec<String>> {
    let mut registration_keys = env
        .iter()
        .filter(|(key, _)| key.starts_with("KOOLADE_MCP_"))
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    registration_keys.sort_unstable();
    let expected_keys = if expected.is_some() {
        vec![
            "KOOLADE_MCP_ARGS",
            "KOOLADE_MCP_COMMAND",
            "KOOLADE_MCP_TOOLS",
        ]
    } else {
        Vec::new()
    };
    assert_eq!(registration_keys, expected_keys);

    let Some(expected) = expected else {
        assert!(argv.iter().any(|arg| arg == "--no-tools"));
        return None;
    };
    let command = env_value(env, "KOOLADE_MCP_COMMAND").to_owned();
    let args: Vec<String> = serde_json::from_str(env_value(env, "KOOLADE_MCP_ARGS")).unwrap();
    let tools: Vec<String> = serde_json::from_str(env_value(env, "KOOLADE_MCP_TOOLS")).unwrap();
    assert_eq!(tools, boundary.mcp_tool_names());
    let registered = std::iter::once(command).chain(args).collect::<Vec<_>>();
    assert_eq!(registered, expected);
    Some(registered)
}

fn arg_after<'a>(argv: &'a [String], option: &str) -> &'a str {
    argv.windows(2)
        .find(|pair| pair[0] == option)
        .map(|pair| pair[1].as_str())
        .unwrap_or_else(|| panic!("provider launch omitted {option}"))
}

fn env_value<'a>(env: &'a [(String, String)], key: &str) -> &'a str {
    env.iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .unwrap_or_else(|| panic!("provider launch omitted {key}"))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
