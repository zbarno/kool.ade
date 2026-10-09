use super::{super::super::ApplicationBoundary, shared::set_option};
use serde_json::json;
use std::fs;

mod config;

pub(super) fn configure(
    boundary: &ApplicationBoundary,
    argv: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    let mut config = config::provider_settings()?;
    let server_name = boundary.mcp_server_name();
    let tool_pattern = format!("{server_name}_*");
    let agent_name = server_name;
    if let Some(args) = boundary.server_args()? {
        config["permission"][&tool_pattern] = json!("allow");
        config["tools"][&tool_pattern] = json!(true);
        config["agent"][agent_name]["permission"][&tool_pattern] = json!("allow");
        config["agent"][agent_name]["tools"][&tool_pattern] = json!(true);
        config["mcp"][server_name] = json!({
            "type": "local",
            "command": args,
            "cwd": boundary.working_directory(),
            "enabled": true,
            "timeout": 1_200_000,
        });
    }
    config["permission"]["*"] = json!("deny");
    config["tools"]["*"] = json!(false);
    config["agent"][agent_name]["permission"]["*"] = json!("deny");
    config["agent"][agent_name]["tools"]["*"] = json!(false);
    config["agent"][agent_name]["mode"] = json!("primary");
    config["default_agent"] = json!(agent_name);
    let path = boundary.scratch.write_json("opencode.json", &config)?;
    let home = boundary.scratch.path.join("opencode-home");
    fs::create_dir(&home)?;
    config::copy_auth(boundary, &home)?;
    let config_dir = home.join("config");
    fs::create_dir(&config_dir)?;
    env.extend([
        ("HOME".into(), home.to_string_lossy().into_owned()),
        (
            "XDG_CONFIG_HOME".into(),
            home.join(".config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            home.join(".local/share").to_string_lossy().into_owned(),
        ),
        (
            "OPENCODE_CONFIG".into(),
            path.to_string_lossy().into_owned(),
        ),
        (
            "OPENCODE_CONFIG_DIR".into(),
            config_dir.to_string_lossy().into_owned(),
        ),
        ("OPENCODE_CONFIG_CONTENT".into(), config.to_string()),
        ("OPENCODE_DISABLE_AUTOUPDATE".into(), "1".into()),
        ("OPENCODE_DISABLE_DEFAULT_PLUGINS".into(), "1".into()),
        ("OPENCODE_DISABLE_CLAUDE_CODE".into(), "1".into()),
        ("OPENCODE_DISABLE_LSP_DOWNLOAD".into(), "1".into()),
        ("OPENCODE_AUTO_SHARE".into(), "false".into()),
    ]);
    set_option(argv, "--agent", agent_name.into());
    set_option(
        argv,
        "--dir",
        boundary.working_directory().to_string_lossy().into_owned(),
    );
    Ok(())
}
