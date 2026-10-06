//! Operator-local coding harness discovery and default selection.

use std::sync::mpsc::{self, Receiver};

mod paint;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeView {
    Pending,
    Report(crate::harness::pi_harness::ProbeReport),
}

pub struct DlgHarnessSetup {
    pub settings: crate::persistence::harness_settings::HarnessSettings,
    pub probe_view: ProbeView,
    pub feedback: Option<(bool, String)>,
    probe_rx: Option<Receiver<crate::harness::pi_harness::ProbeReport>>,
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
            let _ = tx.send(crate::harness::PiHarness::probe_report());
        });
        self.probe_rx = Some(rx);
        self.probe_view = ProbeView::Pending;
        self.feedback = None;
    }

    pub fn drain_probe(&mut self) {
        let Some(rx) = &self.probe_rx else { return };
        match rx.try_recv() {
            Ok(report) => {
                self.probe_rx = None;
                self.record_probe(report);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.probe_rx = None;
                self.feedback = Some((false, "Harness discovery stopped unexpectedly.".into()));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    fn record_probe(&mut self, report: crate::harness::pi_harness::ProbeReport) {
        update_settings_from_probe(&mut self.settings, &report);
        self.probe_view = ProbeView::Report(report);
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

fn update_settings_from_probe(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    report: &crate::harness::pi_harness::ProbeReport,
) {
    let version = (report.ok || report.configuration_required)
        .then(|| report.status.strip_prefix("pi ").map(str::to_owned))
        .flatten();
    settings.discovered.insert(
        "pi".into(),
        crate::persistence::harness_settings::DetectedHarness {
            status: report.status.clone(),
            version,
            executable: report
                .binary
                .as_ref()
                .map(|path| path.to_string_lossy().into()),
            diagnostic: (!report.diagnostic.is_empty()).then_some(report.diagnostic.clone()),
            ready: report.ok,
            configuration_required: report.configuration_required,
        },
    );
    if settings.default_harness.is_none() && report.ok {
        settings.default_harness = Some("pi".into());
    }
}

impl Default for DlgHarnessSetup {
    fn default() -> Self {
        Self::new()
    }
}

pub use paint::paint_harness_setup_card;
