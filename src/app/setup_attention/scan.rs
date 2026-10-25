use super::{SetupIssue, bootstrap, catalog};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub struct SetupScanResult {
    pub generation: u64,
    pub issue: Option<SetupIssue>,
    pub snapshot: Option<catalog::MachineSnapshotV1>,
    pub endpoint_fingerprint: Option<String>,
    pub daemon_identity_fingerprint: Option<String>,
    pub docker_readiness_fingerprint: Option<String>,
    pub selected_provider: Option<catalog::ProviderIdV1>,
    pub cancelled: bool,
}

impl SetupScanResult {
    fn cancelled(generation: u64) -> Self {
        Self {
            generation,
            issue: None,
            snapshot: None,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
            docker_readiness_fingerprint: None,
            selected_provider: None,
            cancelled: true,
        }
    }
}

/// CLI discovery remains independent of Docker. A fresh scan is generation-fenced,
/// bounded, and read-only. The worker persists only after its final identity check.
pub fn scan(
    generation: u64,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> SetupScanResult {
    let started_at = crate::persistence::harness_settings::current_unix_ms();
    let endpoint_before = crate::execution::docker::environment::DockerEnvironmentStatus::
        current_endpoint_fingerprint();
    let (settings, _) = crate::persistence::harness_settings::load();
    let (probes, git_probe) = std::thread::scope(|scope| {
        let git_cancellation = cancellation.clone();
        let git = scope.spawn(move || {
            crate::harness::discovery::with_cancellation(git_cancellation, catalog::probe_git)
        });
        let probes = crate::app::dialogs::probe_bootstrap_clis(generation, cancellation.clone());
        let git_probe = git.join().unwrap_or(catalog::GitProbeV1 {
            executable_state: catalog::EvidenceState::Failed,
            executable_code: catalog::EvidenceCode::GitProbeFailed,
            identity_state: catalog::EvidenceState::Blocked,
            identity_code: catalog::EvidenceCode::ParentPrerequisiteUnavailable,
        });
        (probes, git_probe)
    });
    if cancellation.load(std::sync::atomic::Ordering::Acquire) {
        return SetupScanResult::cancelled(generation);
    }
    let selected = settings.default_harness.as_deref();
    let cli_issue = bootstrap::assess(&probes, selected, generation);
    let docker = crate::execution::docker::environment::DockerEnvironmentStatus::refresh();
    let endpoint_after = crate::execution::docker::environment::DockerEnvironmentStatus::
        current_endpoint_fingerprint();
    if cancellation.load(std::sync::atomic::Ordering::Acquire)
        || endpoint_before != endpoint_after
        || docker.endpoint_fingerprint != endpoint_after
    {
        return SetupScanResult::cancelled(generation);
    }
    let (current_settings, _) = crate::persistence::harness_settings::load();
    if current_settings.default_harness != settings.default_harness {
        return SetupScanResult::cancelled(generation);
    }
    let checked_at = crate::persistence::harness_settings::current_unix_ms();
    let mut snapshot = catalog::assess_for_scan_with_git(
        &probes,
        selected,
        &docker,
        catalog::HostKind::current(),
        checked_at,
        generation,
        git_probe,
    );
    snapshot.scan_started_at_unix_ms = started_at;
    let issue = issue_for_readiness(cli_issue, docker.local_execution_ready(), &snapshot);
    let docker_readiness_fingerprint = docker.readiness_fingerprint();
    SetupScanResult {
        generation,
        issue,
        snapshot: Some(snapshot),
        endpoint_fingerprint: docker.endpoint_fingerprint,
        daemon_identity_fingerprint: docker.daemon_identity_fingerprint,
        docker_readiness_fingerprint: Some(docker_readiness_fingerprint),
        selected_provider: selected.and_then(catalog::ProviderIdV1::from_probe_id),
        cancelled: false,
    }
}

fn issue_for_readiness(
    cli_issue: Option<SetupIssue>,
    local_execution_ready: bool,
    snapshot: &catalog::MachineSnapshotV1,
) -> Option<SetupIssue> {
    let pending_required_task = snapshot.findings.iter().any(|finding| {
        finding.status == catalog::FindingStatus::Unsupported
            || finding.id.starts_with("setup:images:general:")
            || finding.id.starts_with("setup:git:")
    });
    cli_issue.or_else(|| {
        if !local_execution_ready {
            Some(SetupIssue::docker(
                "The container runtime needs attention. Review the specific machine setup task and retry the check.",
            ))
        } else if pending_required_task {
            Some(SetupIssue::bootstrap(
                "A required platform, Git, or general runtime image prerequisite still needs attention.",
            ))
        } else {
            None
        }
    })
}

/// Recheck mutable identities after the scan returns and immediately before
/// the worker persists its snapshot. This remains bounded by
/// the Docker discovery timeout.
#[cfg(not(test))]
pub fn verify_before_commit(
    result: SetupScanResult,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    commit_lock: std::sync::Arc<std::sync::Mutex<()>>,
) -> SetupScanVerification {
    verify_before_commit_at(result, cancellation, commit_lock, None)
}

#[cfg(test)]
pub(crate) fn verify_before_commit_with_state_path(
    result: SetupScanResult,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    commit_lock: std::sync::Arc<std::sync::Mutex<()>>,
    state_path: std::path::PathBuf,
) -> SetupScanVerification {
    verify_before_commit_at(result, cancellation, commit_lock, Some(state_path))
}

fn verify_before_commit_at(
    mut result: SetupScanResult,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    commit_lock: std::sync::Arc<std::sync::Mutex<()>>,
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
    if cancellation.load(std::sync::atomic::Ordering::Acquire)
        || !verification_identity_matches(
            &result,
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
        if let Some(state_path) = state_path.as_deref() {
            crate::persistence::setup_tasks::reconcile_at_if(state_path, snapshot, || {
                !cancellation.load(std::sync::atomic::Ordering::Acquire)
            })
        } else {
            crate::persistence::setup_tasks::reconcile_live_if(snapshot, || {
                !cancellation.load(std::sync::atomic::Ordering::Acquire)
            })
        }
    });
    match persisted {
        Some(Ok(Some(store))) if !cancellation.load(std::sync::atomic::Ordering::Acquire) => {
            SetupScanVerification {
                result,
                machine_setup_tasks: Some(store.tasks.into_values().collect()),
                persistence_failed: false,
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

fn verification_identity_matches(
    result: &SetupScanResult,
    selected: Option<catalog::ProviderIdV1>,
    endpoint_before: Option<String>,
    endpoint_after: Option<String>,
    docker: &crate::execution::docker::environment::DockerEnvironmentStatus,
) -> bool {
    let current_readiness_fingerprint = docker.readiness_fingerprint();
    selected == result.selected_provider
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
