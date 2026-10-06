//! Operator-local coding harness discovery and default selection.

use std::sync::mpsc::{self, Receiver};

mod paint;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeView {
    Pending,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessProbe {
    pub id: String,
    pub status: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub diagnostic: Option<String>,
    pub ready: bool,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub configuration_required: bool,
}

pub struct DlgHarnessSetup {
    pub settings: crate::persistence::harness_settings::HarnessSettings,
    pub probe_view: ProbeView,
    pub feedback: Option<(bool, String)>,
    probe_rx: Option<Receiver<Vec<HarnessProbe>>>,
}

impl DlgHarnessSetup {
    pub fn new() -> Self {
        let (settings, diagnostic) = crate::persistence::harness_settings::load();
        let mut dialog = Self {
            settings,
            probe_view: ProbeView::Pending,
            feedback: diagnostic.map(|message| (false, message)),
            probe_rx: None,
        };
        dialog.refresh();
        dialog
    }

    pub fn refresh(&mut self) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(discover_harnesses());
        });
        self.probe_rx = Some(rx);
        self.probe_view = ProbeView::Pending;
        self.feedback = None;
    }

    pub fn drain_probe(&mut self) {
        let Some(rx) = &self.probe_rx else { return };
        match rx.try_recv() {
            Ok(reports) => {
                self.probe_rx = None;
                self.record_probes(reports);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.probe_rx = None;
                self.feedback = Some((false, "Harness discovery stopped unexpectedly.".into()));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    fn record_probes(&mut self, reports: Vec<HarnessProbe>) {
        apply_probe_results(
            &mut self.settings,
            &reports,
            std::env::var(crate::harness::CODEX_HARNESS_ENV)
                .ok()
                .as_deref(),
        );
        self.probe_view = ProbeView::Complete;
        self.persist();
    }

    pub fn select_default(&mut self, id: &str) {
        if self
            .settings
            .discovered
            .get(id)
            .is_some_and(|harness| harness.ready)
        {
            self.settings.default_harness = Some(id.to_owned());
            self.persist();
        }
    }

    pub fn select_work_route(&mut self, work_type: &str, harness: &str) {
        if self
            .settings
            .discovered
            .get(harness)
            .is_some_and(|entry| entry.ready)
        {
            let model = self
                .settings
                .work_routes
                .get(work_type)
                .and_then(|route| route.model.clone());
            self.settings.work_routes.insert(
                work_type.into(),
                crate::persistence::harness_settings::WorkRoute {
                    harness: harness.into(),
                    model,
                },
            );
            self.persist();
        }
    }

    pub fn set_work_model(&mut self, work_type: &str, model: String) {
        if let Some(route) = self.settings.work_routes.get_mut(work_type) {
            route.model = (!model.trim().is_empty()).then_some(model);
            self.persist();
        }
    }

    fn persist(&mut self) {
        match crate::persistence::harness_settings::save(&self.settings) {
            Ok(()) => self.feedback = Some((true, "Harness settings saved on this device.".into())),
            Err(error) => {
                self.feedback = Some((false, format!("Could not save harness settings: {error}")))
            }
        }
    }
}

fn apply_probe_results(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    reports: &[HarnessProbe],
    preferred: Option<&str>,
) {
    for report in reports {
        update_settings_from_probe(settings, report);
    }
    if settings.default_harness.is_none() {
        settings.default_harness = preferred
            .filter(|id| {
                settings
                    .discovered
                    .get(*id)
                    .is_some_and(|entry| entry.ready)
            })
            .map(str::to_owned)
            .or_else(|| {
                ["pi", "codex", "claude"].into_iter().find_map(|id| {
                    settings
                        .discovered
                        .get(id)
                        .is_some_and(|entry| entry.ready)
                        .then(|| id.to_owned())
                })
            });
    }
}

fn update_settings_from_probe(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    report: &HarnessProbe,
) {
    settings.discovered.insert(
        report.id.clone(),
        crate::persistence::harness_settings::DetectedHarness {
            status: report.status.clone(),
            version: report.version.clone(),
            executable: report.executable.clone(),
            diagnostic: report.diagnostic.clone(),
            ready: report.ready,
            models: report.models.clone(),
            default_model: report.default_model.clone(),
            configuration_required: report.configuration_required,
        },
    );
}

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
        _ => (Vec::new(), None),
    }
}

fn discover_harnesses() -> Vec<HarnessProbe> {
    let pi = crate::harness::PiHarness::probe_report();
    let codex = crate::harness::CodexHarness::probe_report();
    let claude = crate::harness::ClaudeHarness::probe_report();
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
    ]
}

impl Default for DlgHarnessSetup {
    fn default() -> Self {
        Self::new()
    }
}

pub use paint::paint_harness_setup_card;
