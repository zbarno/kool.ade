use super::HarnessProbe;

fn configured_model_catalog(harness: &str) -> (Vec<String>, Option<String>) {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match (harness, home) {
        ("pi", _) => (
            crate::harness::pi_sandbox::configured_provider_models().unwrap_or_default(),
            crate::harness::pi_sandbox::configured_provider_default_model().ok(),
        ),
        ("codex", Some(home)) => {
            let mut models = Vec::new();
            let mut global_default = None;
            let mut active_profile = None;
            let mut profile_models = std::collections::BTreeMap::new();
            let mut section = String::new();
            if let Ok(text) = std::fs::read_to_string(home.join(".codex/config.toml")) {
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        section = trimmed.trim_matches(['[', ']']).to_owned();
                        continue;
                    }
                    let Some((key, value)) = line.split_once('=') else {
                        continue;
                    };
                    let key = key.trim();
                    let model = value.trim().trim_matches(['"', '\'']);
                    if key == "profile" && section.is_empty() {
                        active_profile = Some(model.to_owned());
                    }
                    if key == "model" && !model.is_empty() {
                        models.push(model.to_owned());
                        if section.is_empty() {
                            global_default = Some(model.to_owned());
                        } else if let Some(profile) = section.strip_prefix("profiles.") {
                            profile_models.insert(profile.to_owned(), model.to_owned());
                        }
                    }
                }
            }
            let mut default_model = active_profile
                .and_then(|profile| profile_models.get(&profile).cloned())
                .or(global_default);
            if let Ok(model) = std::env::var(crate::harness::codex_harness::CODEX_MODEL_ENV)
                && !model.trim().is_empty()
            {
                models.push(model.clone());
                default_model = Some(model);
            }
            models.sort();
            models.dedup();
            (models, default_model)
        }
        ("claude", Some(home)) => {
            let configured = std::fs::read(home.join(".claude/settings.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|settings| {
                    settings
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                });
            let mut default_model = configured.clone();
            let mut models = configured.into_iter().collect::<Vec<_>>();
            if let Ok(model) = std::env::var(crate::harness::claude_harness::CLAUDE_MODEL_ENV)
                && !model.trim().is_empty()
            {
                models.push(model.clone());
                default_model = Some(model);
            }
            models.sort();
            models.dedup();
            (models, default_model)
        }
        ("copilot", home) => {
            let configured = std::env::var(crate::harness::copilot_harness::COPILOT_MODEL_ENV)
                .ok()
                .filter(|model| !model.trim().is_empty())
                .or_else(|| {
                    home.and_then(|home| std::fs::read(home.join(".copilot/settings.json")).ok())
                        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                        .and_then(|settings| {
                            settings
                                .get("model")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned)
                        })
                });
            let mut models = vec!["auto".to_owned()];
            if let Some(model) = &configured {
                models.push(model.clone());
            }
            models.sort();
            models.dedup();
            (models, configured)
        }
        _ => (Vec::new(), None),
    }
}

pub(super) fn discover_harnesses() -> Vec<HarnessProbe> {
    let pi = crate::harness::PiHarness::probe_report();
    let codex = crate::harness::CodexHarness::probe_report();
    let claude = crate::harness::ClaudeHarness::probe_report();
    let opencode = crate::harness::OpenCodeHarness::probe_report();
    let copilot = crate::harness::CopilotHarness::probe_report();
    vec![
        HarnessProbe {
            id: "pi".into(),
            version: pi
                .status
                .strip_prefix("pi ")
                .map(str::to_owned)
                .filter(|_| pi.binary.is_some()),
            executable: pi.binary.as_ref().map(|path| path.to_string_lossy().into()),
            diagnostic: (!pi.diagnostic.is_empty()).then_some(pi.diagnostic),
            ready: pi.ok,
            models: configured_model_catalog("pi").0,
            default_model: configured_model_catalog("pi").1,
            configuration_required: pi.configuration_required,
            status: pi.status,
        },
        HarnessProbe {
            id: "codex".into(),
            version: codex.version.clone(),
            executable: codex
                .binary
                .as_ref()
                .map(|path| path.to_string_lossy().into()),
            diagnostic: (!codex.diagnostic.is_empty()).then_some(codex.diagnostic),
            ready: codex.ready,
            models: configured_model_catalog("codex").0,
            default_model: configured_model_catalog("codex").1,
            configuration_required: codex.readiness
                == crate::harness::codex_harness::CodexReadiness::AuthenticationRequired,
            status: codex.status,
        },
        HarnessProbe {
            id: "claude".into(),
            version: claude.version.clone(),
            executable: claude
                .binary
                .as_ref()
                .map(|path| path.to_string_lossy().into()),
            diagnostic: (!claude.diagnostic.is_empty()).then_some(claude.diagnostic),
            ready: claude.readiness == crate::harness::claude_harness::ClaudeReadiness::Ready,
            models: configured_model_catalog("claude").0,
            default_model: configured_model_catalog("claude").1,
            configuration_required: claude.readiness
                == crate::harness::claude_harness::ClaudeReadiness::AuthenticationRequired,
            status: claude.status,
        },
        HarnessProbe {
            id: "opencode".into(),
            version: opencode.version,
            executable: opencode
                .binary
                .as_ref()
                .map(|path| path.to_string_lossy().into()),
            diagnostic: (!opencode.diagnostic.is_empty()).then_some(opencode.diagnostic),
            ready: opencode.readiness == crate::harness::opencode_harness::OpenCodeReadiness::Ready,
            models: opencode.models,
            default_model: None,
            configuration_required: matches!(
                opencode.readiness,
                crate::harness::opencode_harness::OpenCodeReadiness::AuthenticationRequired
                    | crate::harness::opencode_harness::OpenCodeReadiness::ConfigurationRequired
            ),
            status: opencode.status,
        },
        HarnessProbe {
            id: "copilot".into(),
            version: copilot.version.clone(),
            executable: copilot
                .binary
                .as_ref()
                .map(|path| path.to_string_lossy().into()),
            diagnostic: (!copilot.diagnostic.is_empty()).then_some(copilot.diagnostic),
            ready: copilot.readiness == crate::harness::copilot_harness::CopilotReadiness::Ready,
            models: configured_model_catalog("copilot").0,
            default_model: configured_model_catalog("copilot").1,
            configuration_required: false,
            status: match copilot.readiness {
                crate::harness::copilot_harness::CopilotReadiness::Missing => {
                    "copilot (not installed)".into()
                }
                crate::harness::copilot_harness::CopilotReadiness::Unusable => {
                    "copilot (unavailable)".into()
                }
                crate::harness::copilot_harness::CopilotReadiness::Ready => {
                    format!("copilot {}", copilot.version.unwrap_or_default())
                }
            },
        },
    ]
}
