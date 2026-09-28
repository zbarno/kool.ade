//! Fast provider-configuration checks for the board setup issue.
//!
//! This check reads only local settings files. Address resolution remains in
//! the sibling config module on the planning worker, never on the UI path.

use serde_json::Value;
use std::path::Path;

pub(in crate::harness::pi_sandbox::provider_bridge) fn configuration_error() -> Option<String> {
    let home = match std::env::var_os("HOME") {
        Some(home) => std::path::PathBuf::from(home),
        None => return Some("Cannot find Pi home for planning provider".into()),
    };
    configuration_error_from(&home.join(".pi/agent"))
}

pub(super) fn configuration_error_from(agent: &Path) -> Option<String> {
    let result = (|| {
        let settings: Value = super::read_json(&agent.join("settings.json"))?;
        let models: Value = super::read_json(&agent.join("models.json"))?;
        let provider = settings["defaultProvider"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Pi has no default model provider"))?;
        let model_id = settings["defaultModel"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Pi has no default model"))?;
        let definition = &models["providers"][provider];
        ensure_supported_api(definition["api"].as_str())?;
        let base_url = definition["baseUrl"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Pi model provider has no configured base URL"))?;
        validate_base_url_shape(base_url)?;
        let auth: Value = super::read_json(&agent.join("auth.json"))?;
        anyhow::ensure!(
            auth[provider]["key"]
                .as_str()
                .is_some_and(|key| !key.is_empty()),
            "Pi has no saved API key for its selected provider"
        );
        anyhow::ensure!(
            definition["models"].as_array().is_some_and(|models| models
                .iter()
                .any(|model| model["id"].as_str() == Some(model_id))),
            "Pi's selected model is absent from its provider config"
        );
        Ok::<(), anyhow::Error>(())
    })();
    result.err().map(|error| error.to_string())
}

pub(super) fn ensure_supported_api(api: Option<&str>) -> anyhow::Result<()> {
    anyhow::ensure!(
        api == Some("openai-completions"),
        "Planning provider relay currently supports only OpenAI-compatible local providers"
    );
    Ok(())
}

pub(super) fn validate_base_url_shape(base_url: &str) -> anyhow::Result<()> {
    let rest = base_url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("Planning provider must use a bounded HTTP endpoint"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    anyhow::ensure!(
        !authority.is_empty()
            && !authority.contains(['@', '?', '#'])
            && !path.contains(['?', '#', '\\']),
        "Planning provider URL is not a supported base URL"
    );
    if let Some((host, port)) = authority.rsplit_once(':') {
        anyhow::ensure!(
            !host.is_empty() && !host.contains(':') && port.parse::<u16>().is_ok(),
            "Planning provider URL has an invalid host or port"
        );
    } else {
        anyhow::ensure!(
            !authority.contains(':'),
            "IPv6 planning endpoints are not supported"
        );
    }
    let host = authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority);
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        anyhow::ensure!(
            super::local_target(address),
            "Planning provider must resolve to a private/local address"
        );
    }
    Ok(())
}
