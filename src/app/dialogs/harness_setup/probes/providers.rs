use super::{HarnessProbe, catalog::configured_model_catalog, codex_models};

pub const PROVIDERS: [&str; 6] = [
    "pi",
    "codex",
    "claude",
    "opencode",
    "copilot",
    "antigravity",
];

pub fn discover_provider(id: &str, runtime_available: bool) -> HarnessProbe {
    let report = discover_provider_unbounded(id, runtime_available);
    super::bounds::bound_report(report)
}

fn discover_provider_unbounded(id: &str, runtime_available: bool) -> HarnessProbe {
    match id {
        "pi" => {
            let pi = crate::harness::PiHarness::probe_report();
            let (models, default_model) = configured_model_catalog("pi");
            let model_catalog_fresh = !models.is_empty();
            HarnessProbe {
                id: id.into(),
                version: pi
                    .status
                    .strip_prefix("pi ")
                    .map(str::to_owned)
                    .filter(|_| pi.binary.is_some()),
                executable: pi.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!pi.diagnostic.is_empty()).then_some(pi.diagnostic),
                ready: pi.ok,
                models,
                default_model,
                model_catalog_fresh,
                configuration_required: pi.configuration_required,
                implementation_available: pi.ok && runtime_available,
                status: pi.status,
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
            HarnessProbe {
                id: id.into(),
                version: codex.version.clone(),
                executable: codex.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!diagnostic.is_empty()).then(|| diagnostic.join("; ")),
                ready: codex.ready,
                models: merge_models(available_models, configured_models),
                default_model: configured_default.or(available_default),
                model_catalog_fresh,
                configuration_required: codex.readiness
                    == crate::harness::codex_harness::CodexReadiness::AuthenticationRequired,
                implementation_available: codex.ready && runtime_available,
                status: codex.status,
            }
        }
        "claude" => {
            let probe = crate::harness::ClaudeHarness::probe_report();
            let (models, default_model) = configured_model_catalog(id);
            let model_catalog_fresh = !models.is_empty();
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
                implementation_available: ready && runtime_available,
                status: probe.status,
            }
        }
        "opencode" => {
            let probe = crate::harness::OpenCodeHarness::probe_report();
            let ready =
                probe.readiness == crate::harness::opencode_harness::OpenCodeReadiness::Ready;
            let model_catalog_fresh = !probe.models.is_empty();
            HarnessProbe { id: id.into(), version: probe.version, executable: probe.binary.as_ref().map(|p| p.to_string_lossy().into()), diagnostic: (!probe.diagnostic.is_empty()).then_some(probe.diagnostic), ready, models: probe.models, default_model: None, model_catalog_fresh, configuration_required: matches!(probe.readiness, crate::harness::opencode_harness::OpenCodeReadiness::AuthenticationRequired | crate::harness::opencode_harness::OpenCodeReadiness::ConfigurationRequired), implementation_available: ready && runtime_available, status: probe.status }
        }
        "copilot" => {
            let probe = crate::harness::CopilotHarness::probe_report();
            let (models, default_model) = configured_model_catalog(id);
            let model_catalog_fresh = !models.is_empty();
            let ready = probe.readiness == crate::harness::copilot_harness::CopilotReadiness::Ready;
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
                implementation_available: ready && runtime_available,
                status,
            }
        }
        "antigravity" => {
            let probe = crate::harness::AntigravityHarness::probe_report();
            let model_catalog_fresh = !probe.models.is_empty();
            HarnessProbe {
                id: id.into(),
                version: probe.version.clone(),
                executable: probe.binary.as_ref().map(|p| p.to_string_lossy().into()),
                diagnostic: (!probe.diagnostic.is_empty()).then_some(probe.diagnostic),
                ready: probe.ready,
                models: probe.models,
                default_model: std::env::var(
                    crate::harness::antigravity_harness::ANTIGRAVITY_MODEL_ENV,
                )
                .ok()
                .filter(|m| !m.trim().is_empty()),
                model_catalog_fresh,
                configuration_required: false,
                implementation_available: probe.ready && runtime_available,
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
