#[cfg(not(test))]
use super::managed_hooks;
use super::{
    super::super::ApplicationBoundary,
    shared::{set_option, toml_array, toml_string},
};
mod auth;
#[cfg(any(windows, target_os = "macos", test))]
mod managed_policy;
#[cfg(not(test))]
use std::env;
#[cfg(all(not(test), unix))]
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;

#[cfg(all(not(test), unix))]
const SYSTEM_SETTINGS: &[&str] = &[
    "/etc/codex/config.toml",
    "/etc/codex/managed_config.toml",
    "/etc/codex/requirements.toml",
];
#[cfg(all(not(test), unix))]
const SYSTEM_HOOKS: &str = "/etc/codex/hooks.json";

pub(super) fn configure(
    boundary: &ApplicationBoundary,
    argv: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    #[cfg(not(test))]
    {
        #[cfg(unix)]
        {
            managed_hooks::ensure_no_toml_host_execution(
                "Codex",
                SYSTEM_SETTINGS.iter().map(Path::new),
            )?;
            managed_hooks::ensure_no_hook_file(Path::new(SYSTEM_HOOKS), "Codex")?;
        }
        #[cfg(windows)]
        managed_policy::ensure_no_system_host_execution()?;
        #[cfg(target_os = "macos")]
        managed_policy::ensure_no_forced_host_execution_preferences()?;
        if let Some(codex_home) = codex_source_home()? {
            managed_hooks::ensure_no_hook_file(&codex_home.join("hooks.json"), "Codex")?;
        }
    }
    set_option(
        argv,
        "--cd",
        boundary.working_directory().to_string_lossy().into_owned(),
    );
    set_option(argv, "--sandbox", "read-only".into());
    let private_home = boundary.scratch.create_directory("codex-home")?;
    #[cfg(not(test))]
    let auth_source = auth::prepare_auth(
        &boundary.scratch,
        codex_source_home()?.as_deref(),
        &private_home,
        codex_environment_auth_present(),
    )?;
    #[cfg(not(test))]
    boundary.record_codex_auth_strategy(auth_source.into());
    env.push((
        "CODEX_HOME".into(),
        private_home.to_string_lossy().into_owned(),
    ));
    argv.extend([
        "--skip-git-repo-check".into(),
        "--ignore-user-config".into(),
    ]);
    for feature in [
        "shell_tool",
        "unified_exec",
        "unified_exec_tty",
        "shell_snapshot",
        "shell_snapshot_v2",
        "workspace_dependencies",
        "code_mode_host",
        "code_mode",
        "code_mode_only",
        "computer_use",
        "browser_annotation_api",
        "browser_use",
        "browser_use_external",
        "browser_use_full_cdp_access",
        "in_app_browser",
        "in_app_local_automation",
        "view_image",
        "apps",
        "memories",
        "daemon_auto_start",
        "hooks",
        "plugins",
        "skill_search",
        "skill_mcp_dependency_install",
        "multi_agent",
        "multi_agent_v2",
        "multi_agent_v2_dynamic_tools",
        "external_agent_memory_import",
    ] {
        argv.extend(["--disable".into(), feature.into()]);
    }
    let Some(args) = boundary.server_args()? else {
        argv.extend(["--config".into(), "mcp_servers={}".into()]);
        return Ok(());
    };
    let server = boundary.mcp_server_name();
    for (key, value) in [
        (
            format!("mcp_servers.{server}.command"),
            toml_string(&args[0]),
        ),
        (format!("mcp_servers.{server}.args"), toml_array(&args[1..])),
        (format!("mcp_servers.{server}.enabled"), "true".into()),
        (format!("mcp_servers.{server}.required"), "true".into()),
        (
            format!("mcp_servers.{server}.tool_timeout_sec"),
            "1200".into(),
        ),
    ] {
        argv.extend(["--config".into(), format!("{key}={value}")]);
    }
    Ok(())
}

#[cfg(not(test))]
fn codex_source_home() -> anyhow::Result<Option<PathBuf>> {
    let path = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")));
    if let Some(path) = &path {
        anyhow::ensure!(
            path.is_absolute(),
            "Codex configuration directory must be absolute"
        );
    }
    Ok(path)
}

#[cfg(not(test))]
fn codex_environment_auth_present() -> bool {
    [
        "OPENAI_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "OPENAI_FEDERATION_RULE_ID",
        "OPENAI_IDENTITY_TOKEN_FILE",
    ]
    .iter()
    .any(|name| env::var_os(name).is_some_and(|value| !value.to_string_lossy().trim().is_empty()))
}
