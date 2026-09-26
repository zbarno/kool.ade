use std::path::Path;

pub(super) fn summary(repo: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(crate::artifacts::repo_artifact(
        repo,
        crate::artifacts::MCP_CONFIG_FILE,
    ))
    .ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    let config: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(config) => config,
        Err(_) => {
            return Some(
                "MCP configuration is present; its commands and contents are hidden.".into(),
            );
        }
    };
    let names = config
        .get("servers")
        .and_then(serde_json::Value::as_object)
        .map(|servers| servers.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if names.is_empty() {
        return Some(
            "MCP configuration is present; server commands and credentials are hidden.".into(),
        );
    }
    Some(format!(
        "Configured MCP server names (commands, arguments, and credentials are hidden):\n{}",
        names
            .iter()
            .map(|name| format!("- {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_exposes_server_names_but_not_secrets_or_commands() {
        let root = std::env::temp_dir().join(format!("packet-mcp-summary-{}", std::process::id()));
        let path = crate::artifacts::repo_artifact(&root, crate::artifacts::MCP_CONFIG_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"servers":{"catalog":{"command":"private-runner","env":{"TOKEN":"private-token"}},"search":{"command":"hidden-tool"}}}"#,
        )
        .unwrap();
        let output = summary(&root).unwrap();
        assert!(output.contains("catalog"));
        assert!(output.contains("search"));
        assert!(!output.contains("private-runner"));
        assert!(!output.contains("private-token"));
        assert!(!output.contains("hidden-tool"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_mcp_configuration_is_reported_without_echoing_contents() {
        let root = std::env::temp_dir().join(format!(
            "packet-mcp-summary-malformed-{}",
            std::process::id()
        ));
        let path = crate::artifacts::repo_artifact(&root, crate::artifacts::MCP_CONFIG_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "private-token: not-json").unwrap();
        let output = summary(&root).unwrap();
        assert!(output.contains("configuration is present"));
        assert!(!output.contains("private-token"));
        let _ = std::fs::remove_dir_all(root);
    }
}
