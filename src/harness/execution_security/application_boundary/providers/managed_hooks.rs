use std::{fs, path::Path};

const CONFIG_LIMIT: u64 = 1_048_576;

pub(super) fn ensure_no_toml_host_execution<'a>(
    provider: &str,
    paths: impl IntoIterator<Item = &'a Path>,
) -> anyhow::Result<()> {
    for path in paths {
        let Some(contents) = read_optional(path)? else {
            continue;
        };
        let document = contents
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| {
                anyhow::anyhow!(
                    "Cannot inspect {provider} managed settings {}: {error}",
                    path.display()
                )
            })?;
        anyhow::ensure!(
            document.get("hooks").is_none(),
            "{provider} has local administrator-managed hooks in {}; these hooks run on the host outside Kool.ad/e's Bubblewrap boundary",
            path.display()
        );
        anyhow::ensure!(
            document.get("notify").is_none(),
            "{provider} has a local administrator-managed notify command in {}; it runs on the host outside Kool.ad/e's Bubblewrap boundary",
            path.display()
        );
        for key in ["mcp_servers", "plugins"] {
            if let Some(item) = document.get(key) {
                anyhow::ensure!(
                    item.as_table_like()
                        .is_some_and(toml_edit::TableLike::is_empty),
                    "{provider} has local administrator-managed {key} in {}; these tools can run on the host outside Kool.ad/e's Bubblewrap boundary",
                    path.display()
                );
            }
        }
        if let Some(features) = document.get("features") {
            let Some(features) = features.as_table_like() else {
                anyhow::bail!(
                    "Cannot safely inspect {provider} feature settings in {}",
                    path.display()
                );
            };
            for (feature, enabled) in features.iter() {
                anyhow::ensure!(
                    enabled.as_bool() == Some(false),
                    "{provider} has a local administrator-managed feature setting in {}; the enabled or invalid `{feature}` feature could run outside Kool.ad/e's Bubblewrap boundary",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

pub(super) fn ensure_no_hook_file(path: &Path, provider: &str) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => anyhow::bail!(
            "{provider} has a local hook file in {}; its hooks can run on the host outside Kool.ad/e's Bubblewrap boundary",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn ensure_no_json_mcp_servers(path: &Path, provider: &str) -> anyhow::Result<()> {
    let Some(contents) = read_optional(path)? else {
        return Ok(());
    };
    let settings: serde_json::Value = serde_json::from_str(&contents).map_err(|error| {
        anyhow::anyhow!(
            "Cannot inspect {provider} managed MCP settings {}: {error}",
            path.display()
        )
    })?;
    anyhow::ensure!(
        settings.is_object(),
        "Cannot safely inspect {provider} managed MCP settings {}",
        path.display()
    );
    if let Some(servers) = settings.get("mcpServers") {
        anyhow::ensure!(
            servers.as_object().is_some_and(serde_json::Map::is_empty),
            "{provider} has local managed MCP servers in {}; those servers can run tools on the host outside Kool.ad/e's Bubblewrap boundary",
            path.display()
        );
    }
    Ok(())
}

fn read_optional(path: &Path) -> anyhow::Result<Option<String>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= CONFIG_LIMIT,
        "Managed settings file {} is invalid or exceeds the inspection limit",
        path.display()
    );
    Ok(Some(fs::read_to_string(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str, contents: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "koolade-hook-policy-{name}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn codex_local_settings_reject_host_commands_and_extra_tools() {
        let hooks = Fixture::new("codex", "[hooks]\n[[hooks.SessionStart]]\n");
        let error = ensure_no_toml_host_execution("Codex", [hooks.0.as_path()]).unwrap_err();
        assert!(error.to_string().contains("administrator-managed hooks"));

        let safe = Fixture::new("codex-safe", "[features]\nhooks = false\n");
        ensure_no_toml_host_execution("Codex", [safe.0.as_path()]).unwrap();

        for (name, config) in [
            ("notify", "notify = [\"/tmp/hook\"]"),
            ("native-feature", "[features]\nshell_tool = true"),
            (
                "mcp-server",
                "[mcp_servers.untrusted]\ncommand = \"/tmp/server\"",
            ),
            ("plugin", "[plugins.sample]\nenabled = true"),
        ] {
            let config = Fixture::new(name, config);
            assert!(ensure_no_toml_host_execution("Codex", [config.0.as_path()]).is_err());
        }
    }

    #[test]
    fn codex_rejects_user_and_system_hook_files_even_when_user_config_is_ignored() {
        for layer in ["user-home", "system-config"] {
            let directory = std::env::temp_dir().join(format!(
                "koolade-codex-hook-file-{layer}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir(&directory).unwrap();
            let hooks = directory.join("hooks.json");
            fs::write(&hooks, r#"{"hooks":{"SessionStart":[]}}"#).unwrap();
            assert!(ensure_no_hook_file(&hooks, "Codex").is_err(), "{layer}");
            fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn claude_managed_mcp_servers_are_rejected() {
        let servers = Fixture::new(
            "claude-managed-mcp",
            r#"{"mcpServers":{"extra":{"command":"/tmp/server"}}}"#,
        );
        assert!(ensure_no_json_mcp_servers(&servers.0, "Claude Code").is_err());
        let empty = Fixture::new("claude-empty-mcp", r#"{"mcpServers":{}}"#);
        ensure_no_json_mcp_servers(&empty.0, "Claude Code").unwrap();
    }
}
