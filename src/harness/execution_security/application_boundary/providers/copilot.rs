use super::{super::super::ApplicationBoundary, shared::set_option};
use serde_json::json;
#[cfg(not(test))]
use std::path::PathBuf;
use std::{ffi::OsStr, fs, path::Path};

const SYSTEM_POLICY_HOOKS: &str = "/etc/github-copilot/policy.d";

pub(super) fn configure(
    boundary: &ApplicationBoundary,
    argv: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    ensure_no_host_policy_hooks(Path::new(SYSTEM_POLICY_HOOKS))?;
    isolate_home(boundary, env)?;
    let server_name = boundary.mcp_server_name();
    let server = boundary.server_args()?.map(|args| {
        json!({
            "type": "local",
            "command": args[0],
            "args": &args[1..],
            "cwd": boundary.working_directory(),
            "tools": boundary.mcp_tool_names(),
            "timeout": 1_200_000,
            "disableToolCache": true,
        })
    });
    let mut servers = serde_json::Map::new();
    if let Some(server) = server {
        servers.insert(server_name.into(), server);
    }
    let path = boundary
        .scratch
        .write_json("copilot-mcp.json", &json!({ "mcpServers": servers }))?;
    let names = boundary
        .mcp_tool_names()
        .into_iter()
        .map(|tool| format!("{server_name}-{tool}"))
        .collect::<Vec<_>>();
    set_option(argv, "--available-tools", names.join(","));
    argv.extend([
        format!("--additional-mcp-config=@{}", path.display()),
        "--disable-builtin-mcps".into(),
        "--no-custom-instructions".into(),
        "--no-remote".into(),
        "--no-remote-export".into(),
    ]);
    if !names.is_empty() {
        argv.push(format!("--allow-tool={server_name}"));
    }
    Ok(())
}

fn ensure_no_host_policy_hooks(directory: &Path) -> anyhow::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if entry.path().extension() == Some(OsStr::new("json")) {
            anyhow::bail!(
                "GitHub Copilot has machine-wide hook files in {}; these hooks execute on the host outside Kool.ad/e's Bubblewrap boundary",
                directory.display()
            );
        }
    }
    Ok(())
}

fn isolate_home(
    boundary: &ApplicationBoundary,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    let home = boundary.scratch.create_directory("copilot-home")?;
    copy_saved_token(boundary, &home)?;
    env.extend([
        ("COPILOT_HOME".into(), home.to_string_lossy().into_owned()),
        (
            "COPILOT_CACHE_HOME".into(),
            home.join("cache").to_string_lossy().into_owned(),
        ),
    ]);
    Ok(())
}

fn copy_saved_token(
    boundary: &ApplicationBoundary,
    private_home: &std::path::Path,
) -> anyhow::Result<()> {
    #[cfg(test)]
    {
        let _ = (boundary, private_home);
        Ok(())
    }
    #[cfg(not(test))]
    {
        if ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
        {
            return Ok(());
        }
        let Some(source_home) = std::env::var_os("COPILOT_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".copilot")))
        else {
            return Ok(());
        };
        let source = source_home.join("config.json");
        let metadata = match fs::metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            metadata.is_file() && metadata.len() <= 16_384,
            "Copilot saved authentication file exceeds the private import limit"
        );
        let token = fs::read_to_string(source)?;
        import_saved_token(boundary, private_home, &token)
    }
}

fn import_saved_token(
    boundary: &ApplicationBoundary,
    private_home: &Path,
    token: &str,
) -> anyhow::Result<()> {
    let token = token.trim();
    if is_supported_token(token) {
        let relative = private_home
            .join("config.json")
            .strip_prefix(&boundary.scratch.path)?
            .to_string_lossy()
            .into_owned();
        boundary.scratch.write_file(&relative, token.as_bytes())?;
    }
    Ok(())
}

fn is_supported_token(token: &str) -> bool {
    ["gho_", "github_pat_", "ghu_"]
        .iter()
        .any(|prefix| token.starts_with(prefix))
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::ToolAccess;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    #[test]
    fn imports_only_a_supported_plain_text_copilot_token_privately() {
        let boundary = test_boundary();
        let private_home = boundary.scratch.create_directory("copilot-home").unwrap();
        import_saved_token(&boundary, &private_home, "github_pat_synthetic123\n").unwrap();
        let token_path = private_home.join("config.json");
        assert_eq!(
            fs::read_to_string(&token_path).unwrap(),
            "github_pat_synthetic123"
        );
        assert_eq!(
            fs::metadata(token_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn ignores_json_settings_and_unsupported_token_types() {
        let boundary = test_boundary();
        let private_home = boundary.scratch.create_directory("copilot-home").unwrap();
        import_saved_token(
            &boundary,
            &private_home,
            r#"{"trustedFolders":[],"hooks":{}}"#,
        )
        .unwrap();
        import_saved_token(&boundary, &private_home, "ghp_unsupported123").unwrap();
        assert!(!private_home.join("config.json").exists());
    }

    #[test]
    fn rejects_host_policy_hook_files_but_ignores_other_files() {
        let boundary = test_boundary();
        let policy_dir = boundary.scratch.create_directory("policy-hooks").unwrap();
        fs::write(policy_dir.join("readme.txt"), "ignored").unwrap();
        ensure_no_host_policy_hooks(&policy_dir).unwrap();
        fs::write(policy_dir.join("00-policy.json"), "{}").unwrap();
        let error = ensure_no_host_policy_hooks(&policy_dir).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside Kool.ad/e's Bubblewrap boundary")
        );
    }

    fn test_boundary() -> ApplicationBoundary {
        ApplicationBoundary {
            scratch: super::super::super::PrivateDirectory::create("koolade-copilot-test").unwrap(),
            server_name: "koolade_copilot_test".into(),
            app_binary: None,
            server_config: None,
            access: ToolAccess::None,
            _resource_bridge: None,
            _sandbox: None,
            _planning_sandbox: None,
        }
    }
}
