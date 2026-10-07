//! Operator-local coding harness discovery and default selection.

use std::sync::mpsc::{self, Receiver};

mod paint;
mod pi_guide;
#[cfg(test)]
mod pi_guide_tests;
mod probes;
#[cfg(test)]
mod tests;
use probes::discover_harnesses;

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
    pub section: HarnessSettingsSection,
    pub manual_path_drafts: std::collections::BTreeMap<String, String>,
    probe_rx: Option<Receiver<Vec<HarnessProbe>>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HarnessSettingsSection {
    #[default]
    Tools,
    Routing,
}

impl DlgHarnessSetup {
    pub fn new() -> Self {
        let (settings, diagnostic) = crate::persistence::harness_settings::load();
        let mut dialog = Self {
            settings,
            probe_view: ProbeView::Pending,
            feedback: diagnostic.map(|message| (false, message)),
            section: HarnessSettingsSection::Tools,
            manual_path_drafts: std::collections::BTreeMap::new(),
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

    pub fn set_manual_path(&mut self, id: &str, value: String) {
        let value = value.trim().to_owned();
        if value.is_empty() {
            self.feedback = Some((
                false,
                "Enter an executable path, or choose automatic discovery.".into(),
            ));
            return;
        }
        let previous = self.settings.clone();
        self.settings
            .manual_executable_paths
            .insert(id.into(), value);
        if self.persist_saved() {
            self.refresh();
        } else {
            self.settings = previous;
        }
    }

    pub fn reset_manual_path(&mut self, id: &str) {
        let previous = self.settings.clone();
        let previous_draft = self.manual_path_drafts.get(id).cloned();
        self.settings.manual_executable_paths.remove(id);
        // The saved discovery record may point at the removed manual binary.
        // Drop it with the reset so an interrupted refresh cannot label that
        // stale path as auto-detected.
        self.settings.discovered.remove(id);
        self.manual_path_drafts.remove(id);
        if self.persist_saved() {
            self.refresh();
        } else {
            self.settings = previous;
            if let Some(draft) = previous_draft {
                self.manual_path_drafts.insert(id.into(), draft);
            }
        }
    }

    fn persist(&mut self) {
        self.persist_saved();
    }

    fn persist_saved(&mut self) -> bool {
        match crate::persistence::harness_settings::save(&self.settings) {
            Ok(()) => {
                self.feedback = Some((true, "Harness settings saved on this device.".into()));
                true
            }
            Err(error) => {
                self.feedback = Some((false, format!("Could not save harness settings: {error}")));
                false
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
                [
                    "pi",
                    "codex",
                    "claude",
                    "opencode",
                    "copilot",
                    "antigravity",
                ]
                .into_iter()
                .find_map(|id| {
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

impl Default for DlgHarnessSetup {
    fn default() -> Self {
        Self::new()
    }
}

pub use paint::paint_harness_setup_card;
