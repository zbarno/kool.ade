use std::{path::Path, time::Duration};

use crate::execution::contract::ReadinessState;

/// Pi's non-generative auth check reports credential readiness without emitting secrets.
pub(super) fn pi_authentication(binary: &Path, provider: Option<&str>) -> ReadinessState {
    let Some(provider) = provider.filter(|provider| {
        !provider.is_empty() && provider.len() <= 128 && !provider.chars().any(char::is_control)
    }) else {
        return ReadinessState::Unknown;
    };
    let config_directory = match std::env::var_os("PI_CODING_AGENT_DIR") {
        Some(path) => match path.to_str() {
            Some(path) => Some(path.to_owned()),
            None => return ReadinessState::Unknown,
        },
        None => None,
    };
    let environment_overrides = config_directory
        .as_deref()
        .map(|path| vec![("PI_CODING_AGENT_DIR", path)])
        .unwrap_or_default();
    let output = match crate::harness::discovery::run_with_environment(
        &[
            binary.to_string_lossy().into_owned(),
            "auth".into(),
            "check".into(),
            "--provider".into(),
            provider.into(),
            "--json".into(),
            "--no-refresh".into(),
        ],
        Duration::from_secs(10),
        16 * 1024,
        16 * 1024,
        &[],
        &environment_overrides,
    ) {
        Ok(output) if !output.stdout_truncated && !output.stderr_truncated => output,
        _ => return ReadinessState::Unknown,
    };
    let Ok(result) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return ReadinessState::Unknown;
    };
    match result.get("status").and_then(serde_json::Value::as_str) {
        Some("ready") if output.success => ReadinessState::Ready,
        Some("not_ready" | "invalid") => ReadinessState::Unavailable,
        _ => ReadinessState::Unknown,
    }
}

#[cfg(all(test, unix))]
#[path = "authentication/tests.rs"]
mod tests;
