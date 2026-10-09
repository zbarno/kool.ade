#[cfg(not(test))]
use super::managed_hooks;
use super::{super::super::ApplicationBoundary, shared::set_option};
use serde_json::json;
use std::fs;
#[cfg(not(test))]
use std::path::Path;

#[cfg(not(test))]
const SYSTEM_SETTINGS: &str = "/etc/claude-code/managed-settings.json";
#[cfg(not(test))]
const SYSTEM_SETTINGS_DROPINS: &str = "/etc/claude-code/managed-settings.d";
#[cfg(not(test))]
const SYSTEM_MANAGED_MCP: &str = "/etc/claude-code/managed-mcp.json";

pub(super) fn configure(
    boundary: &ApplicationBoundary,
    argv: &mut Vec<String>,
) -> anyhow::Result<()> {
    #[cfg(not(test))]
    ensure_no_local_managed_host_execution()?;
    let server = boundary.server_args()?;
    let mut servers = serde_json::Map::new();
    if let Some(args) = server {
        servers.insert(
            boundary.mcp_server_name().into(),
            json!({ "command": args[0], "args": &args[1..], "cwd": boundary.working_directory() }),
        );
    }
    let path = boundary
        .scratch
        .write_json("claude-mcp.json", &json!({ "mcpServers": servers }))?;
    set_option(argv, "--tools", String::new());
    set_option(argv, "--permission-mode", "dontAsk".into());
    set_option(argv, "--permission-prompts", "none".into());
    argv.extend([
        "--restricted".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        path.to_string_lossy().into_owned(),
    ]);
    let names = boundary
        .mcp_tool_names()
        .into_iter()
        .map(|tool| format!("mcp__{}__{tool}", boundary.mcp_server_name()))
        .collect::<Vec<_>>();
    if !names.is_empty() {
        argv.extend(["--allowedTools".into(), names.join(",")]);
    }
    Ok(())
}

#[cfg(not(test))]
fn ensure_no_local_managed_host_execution() -> anyhow::Result<()> {
    ensure_no_windows_managed_policy_inheritance(is_wsl_environment())?;
    ensure_no_local_managed_settings(
        Path::new(SYSTEM_SETTINGS),
        Path::new(SYSTEM_SETTINGS_DROPINS),
    )?;
    managed_hooks::ensure_no_json_mcp_servers(Path::new(SYSTEM_MANAGED_MCP), "Claude Code")
}

fn ensure_no_local_managed_settings(
    file: &std::path::Path,
    dropins: &std::path::Path,
) -> anyhow::Result<()> {
    reject_managed_settings_path(file)?;
    let entries = match fs::read_dir(dropins) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        reject_managed_settings_path(&entry?.path())?;
    }
    Ok(())
}

fn reject_managed_settings_path(path: &std::path::Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => anyhow::bail!(
            "Claude Code cannot run while local managed settings are present at {}; restricted mode still loads this policy, which can execute host commands outside Kool.ad/e's Bubblewrap boundary",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn ensure_no_windows_managed_policy_inheritance(is_wsl: bool) -> anyhow::Result<()> {
    anyhow::ensure!(
        !is_wsl,
        "Claude Code is unavailable under WSL because it can inherit Windows-managed settings that Kool.ad/e cannot inspect; those settings may launch host commands outside Bubblewrap."
    );
    Ok(())
}

#[cfg(not(test))]
fn is_wsl_environment() -> bool {
    std::env::var_os("WSL_INTEROP").is_some()
        || std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft"))
}

#[cfg(test)]
mod tests {
    use super::{ensure_no_local_managed_settings, ensure_no_windows_managed_policy_inheritance};
    use std::{fs, path::PathBuf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "koolade-claude-managed-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn claude_rejects_windows_managed_policy_inheritance_under_wsl() {
        let error = ensure_no_windows_managed_policy_inheritance(true).unwrap_err();
        assert!(error.to_string().contains("unavailable under WSL"));
    }

    #[test]
    fn claude_allows_linux_without_windows_managed_policy_inheritance() {
        ensure_no_windows_managed_policy_inheritance(false).unwrap();
    }

    #[test]
    fn claude_rejects_any_local_managed_settings_file_or_dropin() {
        let fixture = Fixture::new("settings");
        let settings = fixture.0.join("managed-settings.json");
        let dropins = fixture.0.join("managed-settings.d");
        fs::write(&settings, r#"{"apiKeyHelper":"/tmp/key-helper"}"#).unwrap();
        let error = ensure_no_local_managed_settings(&settings, &dropins).unwrap_err();
        assert!(error.to_string().contains("local managed settings"));

        fs::remove_file(&settings).unwrap();
        fs::create_dir(&dropins).unwrap();
        fs::write(
            dropins.join("org.json"),
            r#"{"awsCredentialExport":"/tmp/credential-export"}"#,
        )
        .unwrap();
        let error = ensure_no_local_managed_settings(&settings, &dropins).unwrap_err();
        assert!(error.to_string().contains("local managed settings"));
    }

    #[test]
    fn claude_allows_missing_local_managed_settings() {
        let fixture = Fixture::new("settings-absent");
        ensure_no_local_managed_settings(
            &fixture.0.join("managed-settings.json"),
            &fixture.0.join("managed-settings.d"),
        )
        .unwrap();
    }
}
