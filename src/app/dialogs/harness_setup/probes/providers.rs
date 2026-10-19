use super::{
    HarnessProbe,
    catalog::{configured_model_catalog, pi_model_catalog},
    codex_models,
};
use crate::execution::contract::ReadinessState;

mod authentication;
mod opencode;
mod opencode_authentication;
mod opencode_local_models;
#[cfg(all(test, unix))]
mod pi_adapter;

pub const PROVIDERS: [&str; 6] = [
    "pi",
    "codex",
    "claude",
    "opencode",
    "copilot",
    "antigravity",
];

#[cfg(all(test, unix))]
#[path = "providers/process_failures.rs"]
mod process_failures;
#[cfg(all(test, unix))]
#[path = "providers/tests.rs"]
mod tests;

pub fn discover_provider(id: &str, runtime_available: bool) -> HarnessProbe {
    let report = discover_provider_unbounded(id, runtime_available);
    super::bounds::bound_report(report)
}

fn discover_provider_unbounded(id: &str, runtime_available: bool) -> HarnessProbe {
    match id {
        "pi" => {
            let pi = crate::harness::PiHarness::probe_report();
            let invalid_explicit_path = pi.binary.is_none()
                && (pi.diagnostic.starts_with("KOOLADE_PI_BIN=")
                    || pi.diagnostic.starts_with("Configured pi executable path")
                    || pi.diagnostic.starts_with("Configured pi path"));
            let status = if invalid_explicit_path {
                "pi (invalid executable path)".to_owned()
            } else {
                pi.status.clone()
            };
            let pi_diagnostic = if invalid_explicit_path {
                "The configured Pi executable path is invalid. Choose a valid executable or reset to auto-detect."
                    .to_owned()
            } else {
                pi.diagnostic
            };
            let (configured_provider, models, default_model, catalog_diagnostic) =
                pi_model_catalog();
            let model_catalog_fresh = !models.is_empty() && catalog_diagnostic.is_none();
            let diagnostic = [
                (!pi_diagnostic.is_empty()).then_some(pi_diagnostic),
                catalog_diagnostic,
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            let authentication = pi
                .binary
                .as_deref()
                .map_or(ReadinessState::Unknown, |binary| {
                    authentication::pi_authentication(binary, configured_provider.as_deref())
                });
            let ready = pi.ok;
            HarnessProbe {
                id: id.into(),
                version: pi
                    .status
                    .strip_prefix("pi ")
                    .map(str::to_owned)
                    .filter(|_| pi.binary.is_some()),
                executable: pi.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!diagnostic.is_empty()).then(|| diagnostic.join("; ")),
                ready,
                models,
                default_model,
                model_catalog_fresh,
                configuration_required: pi.configuration_required,
                authentication,
                implementation_available: ready && runtime_available,
                status,
            }
        }
        "codex" => {
            let codex = crate::harness::CodexHarness::probe_report();
            let (configured_models, configured_default) = configured_model_catalog(id);
            let (available_models, available_default, catalog_diagnostic, catalog_refreshed) =
                match codex.binary.as_deref() {
                    Some(binary) => match codex_models::discover(binary) {
                        Ok((models, default_model)) => (models, default_model, None, true),
                        Err(error) => (
                            Vec::new(),
                            None,
                            Some(format!(
                                "Codex model catalog could not be refreshed: {}",
                                crate::error::redact_secrets(&error.to_string())
                            )),
                            false,
                        ),
                    },
                    None => (Vec::new(), None, None, false),
                };
            let model_catalog_fresh =
                catalog_refreshed || !configured_models.is_empty() && catalog_diagnostic.is_none();
            let diagnostic = [
                (!codex.diagnostic.is_empty()).then_some(codex.diagnostic),
                catalog_diagnostic,
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            let authentication = match codex.readiness {
                crate::harness::codex_harness::CodexReadiness::Ready => ReadinessState::Ready,
                crate::harness::codex_harness::CodexReadiness::AuthenticationRequired => {
                    ReadinessState::Unavailable
                }
                _ => ReadinessState::Unknown,
            };
            let ready = codex.ready;
            HarnessProbe {
                id: id.into(),
                version: codex.version.clone(),
                executable: codex.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!diagnostic.is_empty()).then(|| diagnostic.join("; ")),
                ready,
                models: merge_models(available_models, configured_models),
                default_model: configured_default.or(available_default),
                model_catalog_fresh,
                configuration_required: codex.readiness
                    == crate::harness::codex_harness::CodexReadiness::AuthenticationRequired,
                authentication,
                implementation_available: ready && runtime_available,
                status: codex.status,
            }
        }
        "claude" => {
            let probe = crate::harness::ClaudeHarness::probe_report();
            let (models, default_model) = configured_model_catalog(id);
            let model_catalog_fresh = !models.is_empty();
            let authentication = match probe.readiness {
                crate::harness::claude_harness::ClaudeReadiness::Ready => ReadinessState::Ready,
                crate::harness::claude_harness::ClaudeReadiness::AuthenticationRequired => {
                    ReadinessState::Unavailable
                }
                _ => ReadinessState::Unknown,
            };
            let ready = probe.readiness == crate::harness::claude_harness::ClaudeReadiness::Ready;
            HarnessProbe {
                id: id.into(),
                version: probe.version.clone(),
                executable: probe.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!probe.diagnostic.is_empty()).then_some(probe.diagnostic),
                ready,
                models,
                default_model,
                model_catalog_fresh,
                configuration_required: probe.readiness
                    == crate::harness::claude_harness::ClaudeReadiness::AuthenticationRequired,
                authentication,
                implementation_available: ready && runtime_available,
                status: probe.status,
            }
        }
        "opencode" => opencode::discover(runtime_available),
        "copilot" => {
            let probe = crate::harness::CopilotHarness::probe_report();
            let (models, default_model) = configured_model_catalog(id);
            let model_catalog_fresh = !models.is_empty();
            let probe_ready =
                probe.readiness == crate::harness::copilot_harness::CopilotReadiness::Ready;
            // The supported noninteractive probe only verifies CLI identity/version. Do not
            // infer authentication from that or credential presence; Copilot has no documented
            // non-generative auth check, so it remains installed but unselectable here.
            let authentication = ReadinessState::Unknown;
            let ready = probe_ready;
            let status = match probe.readiness {
                crate::harness::copilot_harness::CopilotReadiness::Missing => {
                    "copilot (not installed)".into()
                }
                crate::harness::copilot_harness::CopilotReadiness::Unusable => {
                    "copilot (unavailable)".into()
                }
                crate::harness::copilot_harness::CopilotReadiness::Ready => {
                    format!("copilot {}", probe.version.clone().unwrap_or_default())
                }
            };
            HarnessProbe {
                id: id.into(),
                version: probe.version.clone(),
                executable: probe.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!probe.diagnostic.is_empty()).then_some(probe.diagnostic),
                ready,
                models,
                default_model,
                model_catalog_fresh,
                configuration_required: false,
                authentication,
                implementation_available: ready && runtime_available,
                status,
            }
        }
        "antigravity" => {
            let probe = crate::harness::AntigravityHarness::probe_report();
            let model_catalog_fresh = !probe.models.is_empty();
            let authentication = if probe.configuration_required {
                ReadinessState::Unavailable
            } else {
                // `agy models` documents catalog listing, not an authentication check. Keep
                // authentication unknown until Antigravity exposes a supported status probe.
                ReadinessState::Unknown
            };
            let ready = probe.ready;
            HarnessProbe {
                id: id.into(),
                version: probe.version.clone(),
                executable: probe.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!probe.diagnostic.is_empty()).then_some(probe.diagnostic),
                ready,
                models: probe.models,
                default_model: std::env::var(
                    crate::harness::antigravity_harness::ANTIGRAVITY_MODEL_ENV,
                )
                .ok()
                .filter(|m| !m.trim().is_empty()),
                model_catalog_fresh,
                configuration_required: probe.configuration_required,
                authentication,
                implementation_available: ready && runtime_available,
                status: probe.status,
            }
        }
        _ => HarnessProbe {
            id: id.to_owned(),
            status: "Unknown provider".into(),
            version: None,
            executable: None,
            diagnostic: Some("Provider is not supported".into()),
            ready: false,
            models: Vec::new(),
            default_model: None,
            model_catalog_fresh: false,
            configuration_required: false,
            authentication: ReadinessState::Unknown,
            implementation_available: false,
        },
    }
}

fn merge_models(mut available: Vec<String>, configured: Vec<String>) -> Vec<String> {
    for model in configured {
        if !available.contains(&model) {
            available.push(model);
        }
    }
    available
}
