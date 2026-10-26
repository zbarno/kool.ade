use std::sync::{Arc, Mutex, atomic::AtomicBool};

use super::SetupScanResult;
use crate::app::setup_attention::catalog;

/// Recheck mutable identities after the scan returns and immediately before
/// the worker persists its snapshot. This remains bounded by
/// the Docker discovery timeout.
#[cfg(not(test))]
pub(crate) fn verify_before_commit(
    result: SetupScanResult,
    cancellation: Arc<AtomicBool>,
    commit_lock: Arc<Mutex<()>>,
) -> SetupScanVerification {
    verify_before_commit_at(result, cancellation, commit_lock, None)
}

#[cfg(test)]
pub(crate) fn verify_before_commit_with_state_path(
    result: SetupScanResult,
    cancellation: Arc<AtomicBool>,
    commit_lock: Arc<Mutex<()>>,
    state_path: std::path::PathBuf,
) -> SetupScanVerification {
    verify_before_commit_at(result, cancellation, commit_lock, Some(state_path))
}

fn verify_before_commit_at(
    mut result: SetupScanResult,
    cancellation: Arc<AtomicBool>,
    commit_lock: Arc<Mutex<()>>,
    state_path: Option<std::path::PathBuf>,
) -> SetupScanVerification {
    if result.cancelled || result.snapshot.is_none() {
        return SetupScanVerification {
            result,
            machine_setup_tasks: None,
            persistence_failed: false,
        };
    }
    // Serialize the final bounded identity check, cancellation check, and local
    // commit with refresh. If refresh wins this lock, it cancels this generation
    // before persistence; if persistence wins, the result was current at commit.
    let _commit_guard = commit_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cancellation.load(std::sync::atomic::Ordering::Acquire) {
        result.cancelled = true;
        return SetupScanVerification {
            result,
            machine_setup_tasks: None,
            persistence_failed: false,
        };
    }
    let endpoint_before = crate::execution::docker::environment::DockerEnvironmentStatus::
        current_endpoint_fingerprint();
    let docker = crate::execution::docker::environment::DockerEnvironmentStatus::refresh();
    let endpoint_after = crate::execution::docker::environment::DockerEnvironmentStatus::
        current_endpoint_fingerprint();
    let (current_settings, _) = crate::persistence::harness_settings::load();
    let current_selected = current_settings
        .default_harness
        .as_deref()
        .and_then(catalog::ProviderIdV1::from_probe_id);
    let current_setup_provider = current_settings
        .bootstrap_provider
        .as_deref()
        .and_then(catalog::ProviderIdV1::from_probe_id);
    if cancellation.load(std::sync::atomic::Ordering::Acquire)
        || !verification_identity_matches(
            &result,
            current_setup_provider,
            current_selected,
            endpoint_before,
            endpoint_after,
            &docker,
        )
    {
        result.cancelled = true;
        return SetupScanVerification {
            result,
            machine_setup_tasks: None,
            persistence_failed: false,
        };
    }
    let persisted = result.snapshot.as_ref().map(|snapshot| {
        if snapshot.scan_generation != result.generation {
            return Err(anyhow::anyhow!(
                "machine setup snapshot generation mismatch"
            ));
        }
        snapshot.validate()?;
        if let Some(state_path) = state_path.as_deref() {
            crate::persistence::setup_tasks::reconcile_at_if(
                state_path,
                snapshot,
                result.generation,
                || !cancellation.load(std::sync::atomic::Ordering::Acquire),
            )
        } else {
            crate::persistence::setup_tasks::reconcile_live_if(snapshot, result.generation, || {
                !cancellation.load(std::sync::atomic::Ordering::Acquire)
            })
        }
    });
    match persisted {
        Some(Ok(Some(store))) if !cancellation.load(std::sync::atomic::Ordering::Acquire) => {
            match crate::persistence::setup_tasks::ordered_task_ids(&store) {
                Ok(ids) => SetupScanVerification {
                    result,
                    machine_setup_tasks: Some(
                        ids.iter()
                            .filter_map(|id| store.tasks.get(id).cloned())
                            .collect(),
                    ),
                    persistence_failed: false,
                },
                Err(_) => SetupScanVerification {
                    result,
                    machine_setup_tasks: None,
                    persistence_failed: true,
                },
            }
        }
        Some(Ok(None)) | Some(Ok(Some(_))) => {
            result.cancelled = true;
            SetupScanVerification {
                result,
                machine_setup_tasks: None,
                persistence_failed: false,
            }
        }
        Some(Err(_)) | None => SetupScanVerification {
            result,
            machine_setup_tasks: None,
            persistence_failed: true,
        },
    }
}

pub(crate) fn verification_identity_matches(
    result: &SetupScanResult,
    setup_provider: Option<catalog::ProviderIdV1>,
    selected: Option<catalog::ProviderIdV1>,
    endpoint_before: Option<String>,
    endpoint_after: Option<String>,
    docker: &crate::execution::docker::environment::DockerEnvironmentStatus,
) -> bool {
    let current_readiness_fingerprint = docker.readiness_fingerprint();
    setup_provider == result.setup_provider
        && selected == result.selected_provider
        && endpoint_before == endpoint_after
        && endpoint_after == result.endpoint_fingerprint
        && docker.endpoint_fingerprint == endpoint_after
        && docker.daemon_identity_fingerprint == result.daemon_identity_fingerprint
        && result.docker_readiness_fingerprint.as_deref()
            == Some(current_readiness_fingerprint.as_str())
}

#[derive(Clone, Debug)]
pub struct SetupScanVerification {
    pub result: SetupScanResult,
    pub machine_setup_tasks: Option<Vec<crate::persistence::setup_tasks::LocalTask>>,
    pub persistence_failed: bool,
}
