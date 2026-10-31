//! App-lifetime, bounded refresh of CLI routing evidence. Discovery and
//! operator choices are different kinds of state: refresh never rewrites routes.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::harness::discovery::{ModelCatalogStatus, ProbeDiagnosticCode, ProbeResult};
use crate::persistence::harness_settings::{self, HarnessSettings, DISCOVERY_FRESHNESS_MS};

use super::{probe_result, probes, discovery::results};

const EARLY_REFRESH_MS: u64 = 3 * 60 * 1000;
const RETRY_INTERVAL: Duration = Duration::from_secs(60);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

struct CompletedScan {
    started_at: u64,
    generation: u64,
    original_paths: BTreeMap<String, String>,
    results: Vec<ProbeResult>,
}

#[derive(Default)]
struct Coordinator {
    worker: Option<JoinHandle<CompletedScan>>,
    last_attempt: Option<Instant>,
    requested: bool,
}

fn coordinator() -> &'static Mutex<Coordinator> {
    static COORDINATOR: OnceLock<Mutex<Coordinator>> = OnceLock::new();
    COORDINATOR.get_or_init(|| Mutex::new(Coordinator::default()))
}

/// An unroutable saved provider requests verification; no probe runs on the
/// caller's thread, and the existing route is never silently changed.
pub(crate) fn request_refresh() {
    coordinator().lock().unwrap_or_else(std::sync::PoisonError::into_inner).requested = true;
}

fn refresh_due(settings: &HarnessSettings, now: u64) -> bool {
    // First run, newly installed providers, and expiring evidence all use
    // the same scheduler. Include absent providers in periodic rescans.
    probes::PROVIDERS.iter().any(|provider| {
        settings.discovered.get(*provider).is_none_or(|entry| {
            !entry.is_fresh_at(now.saturating_add(EARLY_REFRESH_MS))
        })
    })
}

fn scan() -> CompletedScan {
    let (settings, _) = harness_settings::load();
    let started_at = harness_settings::current_unix_ms();
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let cancellation = std::sync::Arc::new(AtomicBool::new(false));
    let runtime_available = std::panic::catch_unwind(
        crate::harness::runtime_capabilities::RuntimeCapabilities::detect,
    ).map(|capabilities| capabilities.implementation).unwrap_or(false);
    let mut results = super::bootstrap_probe::probe_bootstrap_clis(generation, cancellation);
    for result in &mut results {
        result.set_runtime_readiness(runtime_available);
    }
    CompletedScan {
        started_at,
        generation,
        original_paths: settings.manual_executable_paths,
        results,
    }
}

/// Commits only results that have not been superseded by a manual scan or
/// a user changing an executable path while the worker was probing.
/// Reloading before saving preserves defaults, models and work routes.
fn merge_results(settings: &mut HarnessSettings, scan: CompletedScan) -> bool {
    let mut changed = false;
    for mut result in scan.results {
        let id = result.provider_id.clone();
        if !probes::PROVIDERS.contains(&id.as_str())
            || result.diagnostic_code == Some(ProbeDiagnosticCode::InternalProbeFailure)
            || settings.manual_executable_paths.get(&id) != scan.original_paths.get(&id)
            || settings.discovered.get(&id).is_some_and(|previous| {
                previous.checked_at_unix_ms > scan.started_at
            })
        {
            continue;
        }
        // A failed catalog lookup may preserve a last-known display value,
        // but must never turn stale models into selectable verified models.
        if result.model_catalog_status != ModelCatalogStatus::Fresh
            && result.models.is_empty()
            && let Some(previous) = settings.discovered.get(&id)
            && !previous.models.is_empty()
        {
            result.models.clone_from(&previous.models);
            result.default_model.clone_from(&previous.default_model);
            result.model_catalog_status = ModelCatalogStatus::Stale;
        }
        let report = probe_result::into_harness_probe(&result.bounded());
        results::update_settings_from_probe(settings, &report, scan.generation);
        changed = true;
    }
    changed
}

/// Called on normal app ticks, including Welcome. Only one worker runs at a
/// time; a monotonic retry interval prevents an expired record from causing a
/// frame-by-frame probe loop after failures or a system clock adjustment.
pub(crate) fn poll_auto_refresh() {
    let mut state = coordinator().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.worker.as_ref().is_some_and(|worker| !worker.is_finished()) {
        return;
    }
    if let Some(worker) = state.worker.take()
        && let Ok(completed) = worker.join()
    {
        let (mut settings, diagnostic) = harness_settings::load();
        if diagnostic.is_none() && merge_results(&mut settings, completed) {
            if let Err(error) = harness_settings::save(&settings) {
                eprintln!("Kool.ad/e CLI discovery: failed to save refresh: {error}");
            }
        }
    }

    let now = harness_settings::current_unix_ms();
    let settings = harness_settings::load().0;
    if !state.requested && !refresh_due(&settings, now) {
        return;
    }
    if state.last_attempt.is_some_and(|when| when.elapsed() < RETRY_INTERVAL) {
        return;
    }
    state.last_attempt = Some(Instant::now());
    state.requested = false;
    state.worker = Some(std::thread::spawn(scan));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_before_expiration_and_when_no_provider_was_discovered() {
        let now = 1_000_000_000;
        let empty = HarnessSettings::default();
        assert!(refresh_due(&empty, now));
        let mut settings = HarnessSettings::default();
        for id in probes::PROVIDERS {
            settings.discovered.insert(id.to_string(), crate::persistence::harness_settings::DetectedHarness {
                status: "unavailable".into(),
                version: None, executable: None, diagnostic: None, ready: false,
                models: vec![], default_model: None, model_catalog_fresh: false,
                configuration_required: false, implementation_available: false,
                capability_snapshot: None, checked_at_unix_ms: now,
            });
        }
        assert!(!refresh_due(&settings, now));
        assert!(refresh_due(&settings, now + DISCOVERY_FRESHNESS_MS - EARLY_REFRESH_MS + 1));
    }

    #[test]
    fn an_inflight_scan_cannot_replace_changed_manual_path() {
        let mut settings = HarnessSettings::default();
        settings.manual_executable_paths.insert("pi".into(), "/new/pi".into());
        let scan = CompletedScan {
            started_at: 100, generation: 1,
            original_paths: BTreeMap::from([("pi".into(), "/old/pi".into())]),
            results: vec![ProbeResult { provider_id: "pi".into(), ..Default::default() }],
        };
        assert!(!merge_results(&mut settings, scan));
        assert_eq!(settings.manual_executable_paths["pi"], "/new/pi");
    }

    #[test]
    fn failed_background_probe_does_not_delete_saved_routes() {
        let mut settings = HarnessSettings {
            default_harness: Some("pi".into()), ..Default::default()
        };
        settings.work_routes.insert("implementation".into(),
            crate::persistence::harness_settings::WorkRoute {
                harness: "pi".into(), model: Some("chosen".into())
            });
        let scan = CompletedScan {
            started_at: 100, generation: 1, original_paths: BTreeMap::new(),
            results: vec![ProbeResult {
                provider_id: "pi".into(),
                diagnostic_code: Some(ProbeDiagnosticCode::InternalProbeFailure),
                ..Default::default()
            }],
        };
        assert!(!merge_results(&mut settings, scan));
        assert_eq!(settings.default_harness.as_deref(), Some("pi"));
        assert_eq!(settings.work_routes["implementation"].model.as_deref(), Some("chosen"));
    }
}
