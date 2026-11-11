//! App-lifetime, bounded refresh of CLI routing evidence. Discovery and
//! operator choices are different kinds of state: refresh never rewrites routes.
//! Sweeps start once at app start and afterward only on an explicit request
//! (unroutable provider, dialog refresh); nothing polls on a timer.
use std::collections::BTreeMap;
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::JoinHandle;

use crate::harness::discovery::{ModelCatalogStatus, ProbeDiagnosticCode, ProbeResult};
use crate::persistence::harness_settings::{self, HarnessSettings};

use super::{discovery::update_settings_from_probe, probe_result, probes};

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
    requested: bool,
    /// Becomes true once the app-start sweep has retired. Startup sweeps are
    /// exempt from the request requirement; everything after is request-driven.
    startup_sweep_done: bool,
}

fn coordinator() -> &'static Mutex<Coordinator> {
    static COORDINATOR: OnceLock<Mutex<Coordinator>> = OnceLock::new();
    COORDINATOR.get_or_init(|| Mutex::new(Coordinator::default()))
}

/// Whether the app already owns an in-flight sweep. Dialogs avoid starting a
/// competing full scan; results are reconciled into their view after commit.
pub(super) fn refresh_in_flight() -> bool {
    coordinator()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .worker
        .as_ref()
        .is_some_and(|worker| !worker.is_finished())
}

/// An unroutable saved provider requests verification; no probe runs on the
/// caller's thread, and the existing route is never silently changed.
pub(crate) fn request_refresh() {
    coordinator()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .requested = true;
}

/// Decide whether a sweep may start: the one app-start sweep, or any sweep
/// after an explicit request. Nothing starts from elapsed time alone.
fn sweep_should_start(startup_sweep_done: bool, requested: bool) -> bool {
    !startup_sweep_done || requested
}

#[cfg(debug_assertions)]
thread_local! {
    /// Test-build permission for background sweeps; production runs never
    /// consult it. Dormant by default so dialog unit tests neither spawn
    /// probe workers nor consume scan generations by accident.
    static BACKGROUND_SWEEPS_PERMITTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// RAII permission for a test to exercise background sweep admission. Release
/// builds permit sweeps unconditionally; in test builds the scheduler stays
/// dormant on every thread until a test explicitly opts in.
#[allow(dead_code)]
#[cfg(debug_assertions)]
pub(crate) fn with_background_sweeps_permitted<R>(run: impl FnOnce() -> R) -> R {
    let previous = BACKGROUND_SWEEPS_PERMITTED.with(|permit| permit.get());
    BACKGROUND_SWEEPS_PERMITTED.with(|permit| permit.set(true));
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            BACKGROUND_SWEEPS_PERMITTED.with(|permit| permit.set(self.0));
        }
    }
    // The guard is constructed before `run()` so an unwinding closure still
    // restores the previous permit on this (possibly pool-reused) thread
    // instead of leaking dormancy off for later tests.
    let _restore = Restore(previous);
    run()
}

fn background_sweeps_permitted() -> bool {
    #[cfg(debug_assertions)]
    {
        BACKGROUND_SWEEPS_PERMITTED.with(|permit| permit.get())
    }
    #[cfg(not(debug_assertions))]
    {
        true
    }
}

fn scan(original_paths: BTreeMap<String, String>) -> CompletedScan {
    let started_at = harness_settings::current_unix_ms();
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let cancellation = std::sync::Arc::new(AtomicBool::new(false));
    let runtime_available =
        std::panic::catch_unwind(crate::harness::runtime_capabilities::RuntimeCapabilities::detect)
            .map(|capabilities| capabilities.implementation)
            .unwrap_or(false);
    let mut results = super::bootstrap_probe::probe_bootstrap_clis(generation, cancellation);
    for result in &mut results {
        result.set_runtime_readiness(runtime_available);
    }
    CompletedScan {
        started_at,
        generation,
        original_paths,
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
            || settings
                .discovered
                .get(&id)
                .is_some_and(|previous| previous.checked_at_unix_ms > scan.started_at)
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
        update_settings_from_probe(settings, &report, scan.generation);
        changed = true;
    }
    changed
}

/// Called on normal app ticks, including Welcome. Starts the one app-start
/// sweep, then sweeps only while an explicit request is armed. A single
/// worker runs at a time; rapid repeat requests serialize on the worker and
/// the scan-generation/cancellation machinery.
pub(crate) fn poll_auto_refresh() {
    if !background_sweeps_permitted() {
        return;
    }
    let mut state = coordinator()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state
        .worker
        .as_ref()
        .is_some_and(|worker| !worker.is_finished())
    {
        return;
    }
    if let Some(worker) = state.worker.take() {
        match worker.join() {
            Ok(completed) => {
                let (mut settings, diagnostic) = harness_settings::load();
                if diagnostic.is_none()
                    && merge_results(&mut settings, completed)
                    && let Err(error) = harness_settings::save(&settings)
                {
                    eprintln!("Kool.ad/e CLI discovery: failed to save refresh: {error}");
                }
            }
            Err(payload) => {
                eprintln!(
                    "Kool.ad/e CLI discovery: background sweep aborted unexpectedly: {payload:?}"
                );
            }
        }
        // A retired sweep counts as the startup sweep however it ended, so an
        // aborting or panicking scan cannot spin a startup respawn loop.
        state.startup_sweep_done = true;
    }

    if !sweep_should_start(state.startup_sweep_done, state.requested) {
        return;
    }
    // Capture the manual-path guard map on the requesting thread: the
    // sweep worker must not interpret whichever settings store its own
    // thread would resolve.
    let original_paths = harness_settings::load().0.manual_executable_paths;
    state.requested = false;
    state.worker = Some(std::thread::spawn(move || scan(original_paths)));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_sweep_is_one_shot_and_later_sweeps_need_a_request() {
        // The app-start sweep always starts ...
        assert!(sweep_should_start(false, false));
        // ... and so does an explicit request after it has retired ...
        assert!(sweep_should_start(true, true));
        // ... but nothing is scheduled from elapsed time alone.
        assert!(!sweep_should_start(true, false));
        assert!(sweep_should_start(false, true));
    }

    #[test]
    fn an_inflight_scan_cannot_replace_changed_manual_path() {
        let mut settings = HarnessSettings::default();
        settings
            .manual_executable_paths
            .insert("pi".into(), "/new/pi".into());
        let scan = CompletedScan {
            started_at: 100,
            generation: 1,
            original_paths: BTreeMap::from([("pi".into(), "/old/pi".into())]),
            results: vec![ProbeResult {
                provider_id: "pi".into(),
                ..Default::default()
            }],
        };
        assert!(!merge_results(&mut settings, scan));
        assert_eq!(settings.manual_executable_paths["pi"], "/new/pi");
    }

    #[test]
    fn failed_background_probe_does_not_delete_saved_routes() {
        let mut settings = HarnessSettings {
            default_harness: Some("pi".into()),
            ..Default::default()
        };
        settings.work_routes.insert(
            "implementation".into(),
            crate::persistence::harness_settings::WorkRoute {
                harness: "pi".into(),
                model: Some("chosen".into()),
            },
        );
        let scan = CompletedScan {
            started_at: 100,
            generation: 1,
            original_paths: BTreeMap::new(),
            results: vec![ProbeResult {
                provider_id: "pi".into(),
                diagnostic_code: Some(ProbeDiagnosticCode::InternalProbeFailure),
                ..Default::default()
            }],
        };
        assert!(!merge_results(&mut settings, scan));
        assert_eq!(settings.default_harness.as_deref(), Some("pi"));
        assert_eq!(
            settings.work_routes["implementation"].model.as_deref(),
            Some("chosen")
        );
    }
}
