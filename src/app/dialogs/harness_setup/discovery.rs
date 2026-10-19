use std::sync::{atomic::Ordering, mpsc};

use super::probe_result::into_harness_probe;
use super::{DlgHarnessSetup, ProbeView};
use crate::harness::discovery::ProbeResult;

mod results;
pub(super) use results::apply_probe_results;
use results::update_settings_from_probe;
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
                    self.feedback = Some((false, "Harness discovery stopped unexpectedly.".into()));
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
                self.persist();
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
                self.persist();
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
                self.persist();
                self.probe_rx = None;
                self.scan_cancel = None;
                self.probe_view = ProbeView::Complete;
            }
            _ => {}
        }
    }
}
