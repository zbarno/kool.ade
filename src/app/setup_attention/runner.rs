use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::{SetupIssue, SetupScanResult, scan};

/// Recover once at startup, reserve a durable generation, then run the read-only probes.
pub(crate) fn run_scan(
    recover_after_restart: bool,
    cancellation: Arc<AtomicBool>,
    state_path: Option<PathBuf>,
) -> SetupScanResult {
    if cancellation.load(Ordering::Acquire) && !recover_after_restart {
        return SetupScanResult::cancelled(0);
    }
    let reserved = (|| {
        if recover_after_restart {
            if let Some(path) = state_path.as_deref() {
                crate::persistence::setup_tasks::recover_interrupted_at(
                    path,
                    crate::persistence::harness_settings::current_unix_ms(),
                )?;
            } else {
                crate::persistence::setup_tasks::recover_after_restart()?;
            }
        }
        if let Some(path) = state_path.as_deref() {
            crate::persistence::setup_tasks::begin_scan_at(path)
        } else {
            crate::persistence::setup_tasks::begin_scan()
        }
    })();
    match reserved {
        Ok(generation) if !cancellation.load(Ordering::Acquire) => scan(generation, cancellation),
        Ok(generation) => SetupScanResult::cancelled(generation),
        Err(_) => SetupScanResult {
            generation: 0,
            issue: Some(SetupIssue::provider(
                "Could not safely prepare local machine setup state. Retry the check.",
            )),
            snapshot: None,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
            docker_readiness_fingerprint: None,
            selected_provider: None,
            cancelled: false,
        },
    }
}
