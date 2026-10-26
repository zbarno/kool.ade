use super::{SetupIssue, bootstrap, catalog};

mod commit;
mod image_receipts;
#[cfg(test)]
mod tests;

pub use commit::SetupScanVerification;
#[cfg(test)]
pub(crate) use commit::verification_identity_matches;
#[cfg(not(test))]
pub(crate) use commit::verify_before_commit;
#[cfg(test)]
pub(crate) use commit::verify_before_commit_with_state_path;

#[derive(Clone, Debug)]
pub struct SetupScanResult {
    pub generation: u64,
    pub issue: Option<SetupIssue>,
    pub snapshot: Option<catalog::MachineSnapshotV1>,
    pub endpoint_fingerprint: Option<String>,
    pub daemon_identity_fingerprint: Option<String>,
    pub docker_readiness_fingerprint: Option<String>,
    pub setup_provider: Option<catalog::ProviderIdV1>,
    pub selected_provider: Option<catalog::ProviderIdV1>,
    pub cancelled: bool,
}

impl SetupScanResult {
    pub(super) fn cancelled(generation: u64) -> Self {
        Self {
            generation,
            issue: None,
            snapshot: None,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
            docker_readiness_fingerprint: None,
            setup_provider: None,
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
    let setup_provider = settings.bootstrap_provider.as_deref();
    let cli_issue = bootstrap::assess(&probes, selected, setup_provider, generation);
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
    if current_settings.default_harness != settings.default_harness
        || current_settings.bootstrap_provider != settings.bootstrap_provider
    {
        return SetupScanResult::cancelled(generation);
    }
    let checked_at = crate::persistence::harness_settings::current_unix_ms();
    let setup_store = match crate::persistence::setup_tasks::load() {
        Ok(store) => store,
        Err(_) => {
            return SetupScanResult {
                generation,
                issue: Some(SetupIssue::provider(
                    "Could not read workload image requirements. Retry the machine setup check.",
                )),
                snapshot: None,
                endpoint_fingerprint: None,
                daemon_identity_fingerprint: None,
                docker_readiness_fingerprint: None,
                setup_provider: None,
                selected_provider: None,
                cancelled: false,
            };
        }
    };
    let host = catalog::HostKind::current();
    let image_state = image_receipts::scan_state(&setup_store, &docker, host);
    let mut snapshot = catalog::assess_for_scan_with_inputs_and_bootstrap_provider(
        &probes,
        catalog::ProviderChoices {
            default_agent: selected,
            guided_setup: setup_provider,
        },
        &docker,
        host,
        checked_at,
        generation,
        catalog::ScanInputs {
            git_probe,
            images: image_state,
        },
    );
    snapshot.scan_started_at_unix_ms = started_at;
    if snapshot.validate().is_err() {
        return SetupScanResult {
            generation,
            issue: Some(SetupIssue::provider(
                "The machine setup scan produced inconsistent evidence. Retry the check.",
            )),
            snapshot: None,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
            docker_readiness_fingerprint: None,
            setup_provider: None,
            selected_provider: None,
            cancelled: false,
        };
    }
    let issue = issue_for_readiness(cli_issue, docker.local_execution_ready(), &snapshot);
    let docker_readiness_fingerprint = docker.readiness_fingerprint();
    SetupScanResult {
        generation,
        issue,
        snapshot: Some(snapshot),
        endpoint_fingerprint: docker.endpoint_fingerprint,
        daemon_identity_fingerprint: docker.daemon_identity_fingerprint,
        docker_readiness_fingerprint: Some(docker_readiness_fingerprint),
        setup_provider: setup_provider.and_then(catalog::ProviderIdV1::from_probe_id),
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
            || finding.id.starts_with("setup:images:")
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
