//! Account-readiness evidence gathered during provider discovery.

use std::{path::Path, time::Duration};

use crate::execution::contract::ReadinessState;

/// Wall-clock budget for one bounded `pi auth check` probe.
const PI_AUTH_CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// `pi auth check --json` emits a single small document; larger captures are suspect.
const PI_AUTH_CHECK_OUTPUT_LIMIT: usize = 64 * 1024;
/// Longest fragment of an unexpected probe result retained in diagnostics.
const DIAGNOSTIC_TAIL_CHARS: usize = 160;

/// Classified result of one bounded Pi account-readiness probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PiAuthenticationAssessment {
    /// Account accessibility verdict carried into the capability snapshot.
    pub state: ReadinessState,
    /// True when Pi explicitly reported missing sign-in or provider configuration;
    /// surfaced as the "Sign-in required" affordance.
    pub configuration_required: bool,
    /// True only when a live readiness check confirmed the configured provider
    /// during this probe cycle. Consumers may then treat the Pi provider catalog
    /// read from local settings in the same cycle as fresh: the catalog entries
    /// belong to a provider whose account Pi just verified as operable.
    pub live_provider_verified: bool,
    /// Actionable detail merged into the provider's diagnostic list.
    pub diagnostic: Option<String>,
}

/// Classify Pi's configured default provider with a bounded, non-refreshing
/// `pi auth check` probe.
///
/// Trust boundary: the probe invokes the same local Pi installation and the
/// same agent profile directory that the app reads for the model catalog and
/// that Kool.ad/e copies to seed each isolated execution session. `auth check`
/// honors `auth.json` credential resolvers exactly as a real turn does, so
/// verifying readiness does not widen the trust the operator already extended
/// to their local Pi profile. Wall clock, output volume, process supervision,
/// the unsafe-environment denylist, and `--no-refresh` (no OAuth token
/// rotation from discovery) keep the probe bounded and non-mutating.
pub(super) fn evaluate_pi_authentication(
    binary: Option<&Path>,
    provider: Option<&str>,
) -> PiAuthenticationAssessment {
    let Some(binary) = binary else {
        return unverified(None);
    };
    let provider = provider.map(str::trim).unwrap_or("");
    if provider.is_empty() {
        // Absent settings are explained by the model-catalog diagnostic.
        return unverified(None);
    }
    if !is_valid_provider_identifier(provider) {
        return unverified(Some(detail(format!(
            "Pi defaultProvider '{provider}' is not a usable provider identifier; \
             sign-in cannot be verified. Correct the provider in Pi's settings."
        ))));
    }
    let argv = [
        binary.to_string_lossy().into_owned(),
        "auth".into(),
        "check".into(),
        "--provider".into(),
        provider.to_owned(),
        "--json".into(),
        "--no-refresh".into(),
    ];
    // Forward the operator's PI_CODING_AGENT_DIR override so the probe checks
    // the same profile the model catalog reads, not merely $HOME/.pi/agent.
    let agent_dir: Option<String> = std::env::var_os("PI_CODING_AGENT_DIR")
        .and_then(|directory| directory.to_str().map(str::to_owned));
    let agent_dir_override: Vec<(&str, &str)> = match agent_dir.as_deref() {
        Some(directory) => vec![("PI_CODING_AGENT_DIR", directory)],
        None => Vec::new(),
    };
    match crate::harness::discovery::run_with_environment(
        &argv,
        PI_AUTH_CHECK_TIMEOUT,
        PI_AUTH_CHECK_OUTPUT_LIMIT,
        PI_AUTH_CHECK_OUTPUT_LIMIT,
        crate::harness::execution_security::CliProvider::Pi.excluded_child_environment(),
        &agent_dir_override,
    ) {
        Ok(output) => classify(provider, &output),
        Err(error) => unverified(Some(detail(format!(
            "Pi auth check could not be completed: {error}"
        )))),
    }
}

fn classify(
    provider: &str,
    output: &crate::harness::discovery::ProbeOutput,
) -> PiAuthenticationAssessment {
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(document) = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    else {
        return unverified(Some(detail(format!(
            "Pi auth check returned an unrecognized result: {}",
            text.trim()
                .chars()
                .take(DIAGNOSTIC_TAIL_CHARS)
                .collect::<String>()
        ))));
    };
    match document["status"].as_str() {
        // The ready payload is authoritative only on a successful exit; a
        // contradictory result is ambiguous and fails closed.
        Some("ready") if output.success => PiAuthenticationAssessment {
            state: ReadinessState::Ready,
            configuration_required: false,
            live_provider_verified: true,
            diagnostic: None,
        },
        Some("ready") => unverified(Some(detail(
            "Pi auth check reported ready but exited unsuccessfully.".to_owned(),
        ))),
        Some("not_ready") => {
            let reason = document["reason"].as_str().unwrap_or("unspecified");
            let diagnostic = if reason == "provider_not_found" {
                format!(
                    "Pi does not recognize the configured default provider '{provider}'. \
                     Correct the provider in Pi's settings, then rediscover."
                )
            } else {
                format!(
                    "Pi provider '{provider}' requires sign-in ({reason}). Complete sign-in \
                     with the Pi CLI, then rediscover coding tools."
                )
            };
            PiAuthenticationAssessment {
                state: ReadinessState::Unavailable,
                configuration_required: true,
                live_provider_verified: false,
                diagnostic: Some(detail(diagnostic)),
            }
        }
        _ => unverified(Some(detail(format!(
            "Pi auth check reported an unexpected status: {}",
            document["status"].as_str().unwrap_or("missing")
        )))),
    }
}

fn unverified(diagnostic: Option<String>) -> PiAuthenticationAssessment {
    PiAuthenticationAssessment {
        state: ReadinessState::Unknown,
        configuration_required: false,
        live_provider_verified: false,
        diagnostic,
    }
}

/// Pi provider ids are vendor slugs (letters, digits, `.` `_` `+` `-`, and
/// occasional `:`/`/` scoping); rejecting anything else keeps argv predictable.
fn is_valid_provider_identifier(provider: &str) -> bool {
    !(provider.len() > 64 || provider.starts_with('-'))
        && provider.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '_' | '+' | '-' | ':' | '/')
        })
}

/// Uniform shaping for every diagnostic arm: mask credential material
/// first, then clip to the display bound, so truncation can never orphan a
/// partial URL `userinfo` ahead of the `@` that screening relies on.
fn detail(message: String) -> String {
    bounded_message(redact(message))
}

fn redact(message: String) -> String {
    crate::error::redact_secrets(&message)
}

fn bounded_message(message: String) -> String {
    if message.chars().count() <= DIAGNOSTIC_TAIL_CHARS {
        message
    } else {
        format!(
            "{}\u{2026}",
            message
                .chars()
                .take(DIAGNOSTIC_TAIL_CHARS)
                .collect::<String>()
        )
    }
}

#[cfg(all(test, unix))]
#[path = "authentication/tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "authentication/edge/tests.rs"]
mod edge_tests;
