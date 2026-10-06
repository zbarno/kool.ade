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
            configuration_required: report.configuration_required,
        },
    );
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
