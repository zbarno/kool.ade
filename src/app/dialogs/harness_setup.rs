//! Operator-local coding harness discovery and default selection.

use std::sync::mpsc::Receiver;

use crate::harness::discovery::ProbeResult;

mod background;
pub(crate) use background::{
    poll_auto_refresh as poll_cli_auto_refresh, request_refresh as request_cli_auto_refresh,
};
mod bootstrap_probe;
mod bootstrap_provider;
mod discovery;
mod onboarding;
mod paint;
pub use self::paint::paint_harness_setup_page;
mod pi_guide;
#[cfg(test)]
mod pi_guide_tests;
mod probe;
mod probe_result;
mod probes;
#[cfg(test)]
mod review_fixture;
#[cfg(test)]
mod tests;
mod tools;

pub(crate) use bootstrap_probe::probe_bootstrap_clis;
use probe::{HarnessProbe, is_executable_file};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeView {
    Pending,
    Complete,
}

/// How long an operator-feedback flash stays on screen before fading.
const FEEDBACK_VISIBLE_FOR_MS: u64 = 8_000;

#[derive(Clone)]
pub struct DlgHarnessSetup {
    pub settings: crate::persistence::harness_settings::HarnessSettings,
    pub probe_view: ProbeView,
    /// Last operator-feedback flash: (ok, message, set_at_unix_ms).
    pub feedback: Option<(bool, String, u64)>,
    pub section: HarnessSettingsSection,
    pub manual_path_drafts: std::collections::BTreeMap<String, String>,
    probe_rx: Option<std::sync::Arc<std::sync::Mutex<Receiver<discovery::ProbeMessage>>>>,
    scan_generation: u64,
    scan_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    runtime_available: Option<bool>,
    scan_results: Vec<HarnessProbe>,
    pub(super) probe_results: std::collections::BTreeMap<String, ProbeResult>,
    pub(super) running_providers: std::collections::BTreeSet<String>,
    /// True until the dialog's first painted frame has attempted a scan.
    /// Construction itself must never start a sweep: re-created dialogs used
    /// to chain cancel-and-restart faster than any probe could finish. The
    /// first scan arms lazily on the first paint (see
    /// `discovery::ensure_first_scan`) and still obeys the global cooldown.
    first_scan_pending: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HarnessSettingsSection {
    #[default]
    Tools,
    Routing,
}

pub(super) fn invalidate_discovery(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    provider: Option<&str>,
) {
    let Some(provider) = provider else {
        return;
    };
    for (id, entry) in &mut settings.discovered {
        if provider == id {
            entry.checked_at_unix_ms = 0;
            entry.implementation_available = false;
            entry.model_catalog_fresh = false;
            entry.capability_snapshot = None;
        }
    }
}

pub(crate) fn supported_provider_id(id: &str) -> bool {
    probes::PROVIDERS.contains(&id)
}

pub(super) fn has_current_provider_capabilities(
    id: &str,
    detected: &crate::persistence::harness_settings::DetectedHarness,
    now_unix_ms: u64,
) -> bool {
    detected.has_current_authenticated_capability(id, now_unix_ms)
}

pub(super) fn has_current_model_catalog(
    id: &str,
    detected: &crate::persistence::harness_settings::DetectedHarness,
    now_unix_ms: u64,
) -> bool {
    detected.has_current_model_catalog_at(id, now_unix_ms)
}

impl DlgHarnessSetup {
    pub fn new() -> Self {
        let (settings, diagnostic) = crate::persistence::harness_settings::load();
        Self {
            settings,
            probe_view: ProbeView::Pending,
            feedback: diagnostic.map(|message| {
                (
                    false,
                    message,
                    crate::persistence::harness_settings::current_unix_ms(),
                )
            }),
            section: HarnessSettingsSection::Tools,
            manual_path_drafts: std::collections::BTreeMap::new(),
            probe_rx: None,
            scan_generation: 0,
            scan_cancel: None,
            runtime_available: None,
            scan_results: Vec::new(),
            probe_results: Default::default(),
            running_providers: Default::default(),
            // The first scan is armed from the first painted frame, not from
            // the constructor, so a churning dialog lifecycle can no longer
            // mint back-to-back sweep generations before one probe lands.
            first_scan_pending: true,
        }
    }

    pub fn select_default(&mut self, id: &str) {
        if self.settings.discovered.get(id).is_some_and(|harness| {
            has_current_provider_capabilities(
                id,
                harness,
                crate::persistence::harness_settings::current_unix_ms(),
            )
        }) {
            let previous = self.settings.clone();
            self.settings.default_harness = Some(id.to_owned());
            if !self.persist_saved() {
                self.settings = previous;
            }
        } else {
            self.set_feedback(false, format!(
                "{id} cannot be set as the default yet: no fresh authenticated capability evidence exists. Refresh Coding tools, then select it again."
            ));
        }
    }

    pub fn select_work_route(&mut self, work_type: &str, harness: &str) {
        if self.settings.discovered.get(harness).is_some_and(|entry| {
            has_current_provider_capabilities(
                harness,
                entry,
                crate::persistence::harness_settings::current_unix_ms(),
            )
        }) {
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
        } else {
            self.set_feedback(false, format!(
                "Route for {work_type} not changed: {harness} has no fresh authenticated capability evidence. Refresh Coding tools, then choose it again."
            ));
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
        if !probes::PROVIDERS.contains(&id) {
            self.set_feedback(false, format!("Unsupported provider: {id}"));
            return;
        }
        if value.is_empty() {
            self.set_feedback(
                false,
                "Enter an executable path, or choose automatic discovery.".into(),
            );
            return;
        }
        let path = std::path::Path::new(&value);
        let canonical = path.canonicalize().ok();
        if canonical
            .as_deref()
            .is_none_or(|path| !is_executable_file(path))
        {
            self.set_feedback(false, "That path is not an executable file. Choose a valid tool path or return to automatic detection.".into());
            return;
        }
        let value = canonical
            .expect("validated canonical executable path")
            .display()
            .to_string();
        let previous = self.settings.clone();
        self.settings
            .manual_executable_paths
            .insert(id.into(), value);
        invalidate_discovery(&mut self.settings, Some(id));
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

    /// Machine-driven persistence (probe traffic) must not raise the
    /// operator-facing saved banner; failures stay visible.
    fn persist_quiet(&mut self) {
        if let Err(error) = crate::persistence::harness_settings::save(&self.settings) {
            self.set_feedback(false, format!("Could not save harness settings: {error}"));
        }
    }

    fn set_feedback(&mut self, ok: bool, message: String) {
        self.feedback = Some((
            ok,
            message,
            crate::persistence::harness_settings::current_unix_ms(),
        ));
    }

    /// Drops operator feedback that has exceeded its visibility window.
    /// Called at the top of every paint surface so modal and settings-page
    /// presentations age out identically.
    pub fn drop_expired_feedback(&mut self) {
        if let Some((_, _, since)) = &self.feedback
            && crate::persistence::harness_settings::current_unix_ms().saturating_sub(*since)
                >= FEEDBACK_VISIBLE_FOR_MS
        {
            self.feedback = None;
        }
    }

    fn persist_saved(&mut self) -> bool {
        match crate::persistence::harness_settings::save(&self.settings) {
            Ok(()) => {
                self.set_feedback(true, "Harness settings saved on this device.".into());
                true
            }
            Err(error) => {
                self.set_feedback(false, format!("Could not save harness settings: {error}"));
                false
            }
        }
    }
}

impl Default for DlgHarnessSetup {
    fn default() -> Self {
        Self::new()
    }
}

pub use paint::paint_harness_setup_card;
