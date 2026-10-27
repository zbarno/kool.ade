use std::{path::Path, time::Duration};

use crate::execution::contract::ReadinessState;

mod anthropic;

const AUTH_TIMEOUT: Duration = Duration::from_secs(8);
const AUTH_OUTPUT_LIMIT: usize = 16 * 1024;

pub(super) struct RemoteEvidence {
    pub(super) authentication: ReadinessState,
    pub(super) models: Option<Vec<String>>,
    pub(super) model_catalog_fresh: bool,
    pub(super) diagnostic: Option<&'static str>,
}

pub(super) fn verify_anthropic(binary: &Path, catalog_models: &[String]) -> RemoteEvidence {
    let has_selectable_anthropic_model = catalog_models.iter().any(|model| {
        model
            .split_once('/')
            .is_some_and(|(provider, _)| provider == "anthropic")
    });
    if !has_selectable_anthropic_model {
        return RemoteEvidence {
            authentication: probe(binary),
            models: None,
            model_catalog_fresh: false,
            diagnostic: None,
        };
    }
    if crate::harness::execution_security::opencode_has_custom_provider_settings("anthropic")
        .unwrap_or(true)
    {
        return RemoteEvidence {
            authentication: ReadinessState::Unknown,
            models: None,
            model_catalog_fresh: false,
            diagnostic: Some(
                "OpenCode has custom Anthropic provider settings. Remove them before Kool.ad/e can verify the official Anthropic route.",
            ),
        };
    }
    match crate::harness::opencode_harness::export_api_key(binary, "anthropic") {
        crate::harness::opencode_harness::AuthExport::NoSavedCredential => RemoteEvidence {
            authentication: ReadinessState::Unavailable,
            models: None,
            model_catalog_fresh: false,
            diagnostic: Some(
                "No saved Anthropic API key was found. Sign in with `opencode auth login anthropic --method key`, then rediscover.",
            ),
        },
        crate::harness::opencode_harness::AuthExport::Unsupported => {
            let authentication = probe(binary);
            RemoteEvidence {
                authentication,
                models: None,
                model_catalog_fresh: false,
                diagnostic: Some(if authentication == ReadinessState::Unavailable {
                    "OpenCode has no saved credentials. Sign in with `opencode auth login anthropic --method key`, then rediscover."
                } else {
                    "OpenCode authentication could not be verified. Use a saved Anthropic API key with OpenCode v2, then rediscover."
                }),
            }
        }
        crate::harness::opencode_harness::AuthExport::ApiKey(key) => {
            match anthropic::verify_api_key(&key, catalog_models) {
                anthropic::Verification::Verified(models) => RemoteEvidence {
                    authentication: ReadinessState::Ready,
                    models: Some(models),
                    model_catalog_fresh: true,
                    diagnostic: None,
                },
                anthropic::Verification::Rejected => RemoteEvidence {
                    authentication: ReadinessState::Unavailable,
                    models: None,
                    model_catalog_fresh: false,
                    diagnostic: Some(
                        "Anthropic rejected the saved API key. Update it in OpenCode, then rediscover.",
                    ),
                },
                anthropic::Verification::Unknown => RemoteEvidence {
                    authentication: ReadinessState::Unknown,
                    models: None,
                    model_catalog_fresh: false,
                    diagnostic: Some(
                        "Anthropic access could not be verified. Check the network and provider settings, then retry discovery.",
                    ),
                },
            }
        }
    }
}

/// Inspect only whether OpenCode reports configured credentials. Stored credentials do not prove
/// that the account or provider currently accepts requests, so any non-empty result stays unknown.
pub(super) fn probe(binary: &Path) -> ReadinessState {
    let argv = [
        binary.to_string_lossy().into_owned(),
        "--pure".into(),
        "auth".into(),
        "list".into(),
    ];
    let mut output = match crate::harness::discovery::run(
        &argv,
        AUTH_TIMEOUT,
        AUTH_OUTPUT_LIMIT,
        AUTH_OUTPUT_LIMIT,
        crate::harness::execution_security::CliProvider::OpenCode.excluded_child_environment(),
    ) {
        Ok(output) if output.success && !output.stdout_truncated && !output.stderr_truncated => {
            output
        }
        Ok(output) if output.stdout_truncated || output.stderr_truncated => {
            return ReadinessState::Unknown;
        }
        Ok(output) if !output.success => return ReadinessState::Unknown,
        Err(_) => return ReadinessState::Unknown,
        Ok(_) => return ReadinessState::Unknown,
    };
    let result = parse_auth_list(&output.stdout);
    output.stdout.fill(0);
    output.stderr.fill(0);
    result
}

fn parse_auth_list(output: &[u8]) -> ReadinessState {
    let Ok(output) = std::str::from_utf8(output) else {
        return ReadinessState::Unknown;
    };
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        Credentials,
        Environment,
    }
    let mut section = None;
    let mut credential_count = None;
    let mut environment_count = None;
    let mut credential_rows = 0_u64;
    let mut environment_rows = 0_u64;
    for line in output.lines() {
        let body = line_body(line);
        if body.is_empty() {
            continue;
        }
        if let Some(path) = body.strip_prefix("Credentials ") {
            if section.is_some() || path.trim().is_empty() || credential_count.is_some() {
                return ReadinessState::Unknown;
            }
            section = Some(Section::Credentials);
            continue;
        }
        if body == "Environment" {
            if section != Some(Section::Credentials)
                || credential_count.is_none()
                || environment_count.is_some()
            {
                return ReadinessState::Unknown;
            }
            section = Some(Section::Environment);
            continue;
        }
        if let Some(count) = summary_count(body, "credential", "credentials") {
            if section != Some(Section::Credentials) || credential_count.replace(count).is_some() {
                return ReadinessState::Unknown;
            }
            continue;
        }
        if let Some(count) = summary_count(body, "environment variable", "environment variables") {
            if section != Some(Section::Environment) || environment_count.replace(count).is_some() {
                return ReadinessState::Unknown;
            }
            continue;
        }
        if body
            .strip_prefix("● ")
            .is_some_and(|label| !label.trim().is_empty())
        {
            let row_count = match section {
                Some(Section::Credentials) if credential_count.is_none() => &mut credential_rows,
                Some(Section::Environment) if environment_count.is_none() => &mut environment_rows,
                _ => return ReadinessState::Unknown,
            };
            let Some(next_count) = row_count.checked_add(1) else {
                return ReadinessState::Unknown;
            };
            *row_count = next_count;
            continue;
        }
        return ReadinessState::Unknown;
    }
    match (section, credential_count, environment_count) {
        (Some(Section::Environment), Some(credentials), Some(environment))
            if credentials == credential_rows
                && environment == environment_rows
                && (credentials > 0 || environment > 0) =>
        {
            ReadinessState::Unknown
        }
        (Some(Section::Environment), Some(0), Some(0))
            if credential_rows == 0 && environment_rows == 0 =>
        {
            ReadinessState::Unavailable
        }
        _ => ReadinessState::Unknown,
    }
}

fn summary_count(line: &str, singular: &str, plural: &str) -> Option<u64> {
    let line = line.trim();
    let mut fields = line.split_whitespace();
    let count = fields.next()?.parse().ok()?;
    let label = fields.collect::<Vec<_>>().join(" ");
    let expected = if count == 1 { singular } else { plural };
    (label == expected).then_some(count)
}

fn line_body(line: &str) -> &str {
    line.trim().trim_start_matches(['┌', '└', '├', '│']).trim()
}

#[cfg(test)]
#[path = "opencode_authentication/tests.rs"]
mod tests;
