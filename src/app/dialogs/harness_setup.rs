//! Operator-local coding harness discovery and default selection.

use std::sync::mpsc::Receiver;

use crate::harness::discovery::ProbeResult;

mod discovery;
mod paint;
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

use probe::{HarnessProbe, is_executable_file};

/// Reuse Coding Tools' bounded provider probes during first-run setup, before
/// Docker has been configured. Each provider runs independently; a failed or
/// panicking probe must not hide another installed CLI. This is discovery only:
/// it does not select a provider or authorize repository tools.
pub(crate) fn probe_bootstrap_clis() -> Vec<ProbeResult> {
    let mut results = Vec::with_capacity(probes::PROVIDERS.len());
    // Keep resource consumption bounded while preserving independent results.
    for batch in probes::PROVIDERS.chunks(3) {
        std::thread::scope(|scope| {
            let handles = batch
                .iter()
                .map(|&id| {
                    scope.spawn(move || {
                        let start = std::time::Instant::now();
                        let checked = crate::persistence::harness_settings::current_unix_ms();
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            probes::discover_provider(id, false)
                        })) {
                            Ok(report) => probe_result::from_harness_probe(
                                &report,
                                0,
                                checked,
                                start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                            ),
                            Err(_) => probe_result::internal_failure_result(
                                id,
                                0,
                                checked,
                                start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                            ),
                        }
                    })
                })
                .collect::<Vec<_>>();
            for (id, handle) in batch.iter().zip(handles) {
                results.push(handle.join().unwrap_or_else(|_| {
                    probe_result::internal_failure_result(
                        id,
                        0,
                        crate::persistence::harness_settings::current_unix_ms(),
                        0,
                    )
                }));
            }
        });
    }
    results
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeView {
    Pending,
    Complete,
}

#[derive(Clone)]
pub struct DlgHarnessSetup {
    pub settings: crate::persistence::harness_settings::HarnessSettings,
    pub probe_view: ProbeView,
    pub feedback: Option<(bool, String)>,
    pub section: HarnessSettingsSection,
    pub manual_path_drafts: std::collections::BTreeMap<String, String>,
    probe_rx: Option<std::sync::Arc<std::sync::Mutex<Receiver<discovery::ProbeMessage>>>>,
    scan_generation: u64,
    scan_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    runtime_available: Option<bool>,
    scan_results: Vec<HarnessProbe>,
    pub(super) probe_results: std::collections::BTreeMap<String, ProbeResult>,
    pub(super) running_providers: std::collections::BTreeSet<String>,
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
    detected.is_fresh_at(now_unix_ms)
        && detected.model_catalog_fresh
        && detected
            .capability_snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.provider_id == id
                    && snapshot.validate().is_ok()
                    && snapshot.model_catalog == crate::execution::contract::ReadinessState::Ready
            })
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
            scan_generation: 0,
            scan_cancel: None,
            runtime_available: None,
            scan_results: Vec::new(),
            probe_results: Default::default(),
            running_providers: Default::default(),
        };
        dialog.refresh();
        dialog
    }

    pub fn select_default(&mut self, id: &str) {
        if self.settings.discovered.get(id).is_some_and(|harness| {
            has_current_provider_capabilities(
                id,
                harness,
                crate::persistence::harness_settings::current_unix_ms(),
            )
        }) {
            self.settings.default_harness = Some(id.to_owned());
            self.persist();
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
            self.feedback = Some((false, format!("Unsupported provider: {id}")));
            return;
        }
        if value.is_empty() {
            self.feedback = Some((
                false,
                "Enter an executable path, or choose automatic discovery.".into(),
            ));
            return;
        }
        let path = std::path::Path::new(&value);
        let canonical = path.canonicalize().ok();
        if canonical
            .as_deref()
            .is_none_or(|path| !is_executable_file(path))
        {
            self.feedback = Some((
                false,
                "That path is not an executable file. Choose a valid tool path or return to automatic detection.".into(),
            ));
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

pub fn paint_harness_setup_page(
    ui: &mut egui::Ui,
    dialog: &mut DlgHarnessSetup,
    section: HarnessSettingsSection,
) {
    dialog.drain_probe();
    match section {
        HarnessSettingsSection::Tools => paint::paint_tools(ui, dialog),
        HarnessSettingsSection::Routing => paint::paint_work_routes(ui, dialog),
    }
    if let Some((ok, message)) = &dialog.feedback {
        ui.colored_label(
            if *ok {
                crate::ui::theme::SUCCESS
            } else {
                crate::ui::theme::DANGER
            },
            message,
        );
    }
}

impl Default for DlgHarnessSetup {
    fn default() -> Self {
        Self::new()
    }
}

pub use paint::paint_harness_setup_card;
