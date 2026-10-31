use std::sync::{atomic::Ordering, mpsc};

use super::probe_result::into_harness_probe;
use super::{DlgHarnessSetup, ProbeView};
use crate::harness::discovery::ProbeResult;

pub(super) mod cooldown;
use cooldown::{ScanWindow, acquire_scan_window};
mod results;
pub(super) use results::apply_probe_results;
#[cfg(test)]
pub(super) use results::update_runtime_readiness;
pub(in crate::app::dialogs::harness_setup) use results::update_settings_from_probe;
mod workers;

#[cfg(test)]
pub(crate) mod test_hooks;

#[derive(Debug)]
pub(super) enum ProbeMessage {
    Provider {
        generation: u64,
        result: ProbeResult,
    },
    Started {
        generation: u64,
        provider_id: String,
    },
    Runtime {
        generation: u64,
        implementation_available: bool,
    },
    Finished {
        generation: u64,
    },
}

impl DlgHarnessSetup {
    pub fn cancel_discovery(&mut self) {
        if let Some(cancel) = self.scan_cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        self.probe_rx = None;
        self.running_providers.clear();
        self.probe_view = ProbeView::Complete;
    }

    /// Arm this dialog's first-ever scan on the first painted frame.
    ///
    /// Freshness, not eagerness, decides: if the persisted discovery records
    /// already carry evidence younger than the shared freshness horizon the
    /// cached view is served as-is (close/reopen does not burn another
    /// sweep) and the flag is consumed silently. Otherwise a scan is
    /// attempted through the global cooldown, re-trying on each painted
    /// frame until the compare-and-swap admits it; a declined attempt keeps
    /// the flag pending. Later frames never re-arm once consumed.
    ///
    /// Known trade-off: if a provider vanishes while its record is still
    /// younger than the horizon, the cached row stays visible for at most
    /// that horizon; the explicit Rediscover button bypasses the freshness
    /// gate (it only honours the scan-window cooldown), so an operator can
    /// force the correction immediately.
    pub fn ensure_first_scan(&mut self) {
        if !self.first_scan_pending {
            return;
        }
        if self.has_recent_discovery_evidence() {
            self.first_scan_pending = false;
            self.probe_view = ProbeView::Complete;
            return;
        }
        if matches!(
            acquire_scan_window(cooldown::effective_now_ms()),
            ScanWindow::Accepted
        ) {
            self.first_scan_pending = false;
            self.launch_full_scan();
        }
    }

    /// Whether every known or configured provider has recent persisted
    /// evidence. An unrelated fresh provider must not suppress refresh of a
    /// stale route, or the router will continue to reject work after the
    /// operator follows the refresh guidance.
    fn has_recent_discovery_evidence(&self) -> bool {
        let now = cooldown::effective_now_ms();
        let mut routed = self
            .settings
            .work_routes
            .values()
            .map(|route| route.harness.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        routed.extend(self.settings.discovered.keys().map(String::as_str));
        if let Some(default) = self.settings.default_harness.as_deref().or_else(|| {
            (std::env::var(crate::harness::CODEX_HARNESS_ENV).as_deref() == Ok("codex"))
                .then_some("codex")
        }) {
            routed.insert(default);
        }

        !routed.is_empty()
            && routed.into_iter().all(|provider| {
                self.settings
                    .discovered
                    .get(provider)
                    .is_some_and(|entry| entry.is_fresh_at(now))
            })
    }

    /// Refresh only evidence from the background coordinator. Never replace
    /// operator choices or a live dialog-owned scan with a stale settings copy.
    pub(super) fn reload_newer_discovery(&mut self) {
        if self.probe_rx.is_some() {
            return;
        }
        let (saved, diagnostic) = crate::persistence::harness_settings::load();
        if diagnostic.is_some() {
            return;
        }
        for (id, entry) in saved.discovered {
            if self.settings.discovered.get(&id).is_none_or(|current| {
                entry.checked_at_unix_ms > current.checked_at_unix_ms
            }) {
                self.settings.discovered.insert(id, entry);
            }
        }
    }

    pub fn drain_probe(&mut self) {
        let Some(rx) = self.probe_rx.as_ref().cloned() else {
            return;
        };
        loop {
            let result = match rx.lock() {
                Ok(rx) => rx.try_recv(),
                Err(_) => Err(mpsc::TryRecvError::Disconnected),
            };
            match result {
                Ok(message) => self.apply_probe_message(message),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.probe_rx = None;
                    self.set_feedback(false, "Harness discovery stopped unexpectedly.".into());
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
    }

    pub(super) fn apply_probe_message(&mut self, message: ProbeMessage) {
        match message {
            ProbeMessage::Started {
                generation,
                provider_id,
            } if generation == self.scan_generation => {
                self.running_providers.insert(provider_id);
            }
            ProbeMessage::Provider {
                generation,
                mut result,
            } if generation == self.scan_generation => {
                let preserve_last_known = result.diagnostic_code
                    == Some(crate::harness::discovery::ProbeDiagnosticCode::InternalProbeFailure);
                if result.model_catalog_status
                    != crate::harness::discovery::ModelCatalogStatus::Fresh
                    && result.models.is_empty()
                    && let Some(previous) = self.settings.discovered.get(&result.provider_id)
                    && !previous.models.is_empty()
                {
                    result.models.clone_from(&previous.models);
                    result.default_model.clone_from(&previous.default_model);
                    result.model_catalog_status =
                        crate::harness::discovery::ModelCatalogStatus::Stale;
                    result = result.bounded();
                }
                if let Some(runtime_available) = self.runtime_available {
                    result.set_runtime_readiness(runtime_available);
                }
                let report = into_harness_probe(&result);
                if !preserve_last_known {
                    update_settings_from_probe(&mut self.settings, &report, generation);
                }
                if !preserve_last_known
                    && let Some(runtime_available) = self.runtime_available
                    && let Some(entry) = self.settings.discovered.get_mut(&report.id)
                    && let Some(snapshot) = entry.capability_snapshot.as_mut()
                {
                    snapshot.execution_target = if runtime_available {
                        crate::execution::contract::ReadinessState::Ready
                    } else {
                        crate::execution::contract::ReadinessState::Unavailable
                    };
                    entry.implementation_available = report.ready && runtime_available;
                }
                self.running_providers.remove(&result.provider_id);
                self.probe_results
                    .insert(result.provider_id.clone(), result);
                if !preserve_last_known {
                    self.scan_results.push(report);
                }
                self.persist_quiet();
            }
            ProbeMessage::Runtime {
                generation,
                implementation_available,
            } if generation == self.scan_generation => {
                self.runtime_available = Some(implementation_available);
                results::update_runtime_readiness(
                    &mut self.settings,
                    &mut self.scan_results,
                    implementation_available,
                );
                for result in self.probe_results.values_mut() {
                    result.set_runtime_readiness(implementation_available);
                }
                for entry in self.settings.discovered.values_mut() {
                    entry.implementation_available = entry.ready && implementation_available;
                }
                self.persist_quiet();
            }
            ProbeMessage::Finished { generation } if generation == self.scan_generation => {
                self.running_providers.clear();
                apply_probe_results(
                    &mut self.settings,
                    &self.scan_results,
                    std::env::var(crate::harness::CODEX_HARNESS_ENV)
                        .ok()
                        .as_deref(),
                    generation,
                );
                self.persist_quiet();
                self.probe_rx = None;
                self.scan_cancel = None;
                self.probe_view = ProbeView::Complete;
            }
            _ => {}
        }
    }
}
