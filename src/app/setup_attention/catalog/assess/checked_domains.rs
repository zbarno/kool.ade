use crate::execution::docker::environment::DockerEnvironmentStatus;
use crate::harness::discovery::{ProbeResult, ProbeStatus};

use super::{EvidenceState, GitProbeV1, HostKind, ImageScanState, images};

pub(super) struct Inputs<'a> {
    pub(super) probes: &'a [ProbeResult],
    pub(super) scan_generation: u64,
    pub(super) verified: &'a [&'a ProbeResult],
    pub(super) selected_agent: Option<&'a str>,
    pub(super) docker: &'a DockerEnvironmentStatus,
    pub(super) host: HostKind,
    pub(super) git_probe: &'a GitProbeV1,
    pub(super) image_state: &'a ImageScanState,
}

pub(super) fn collect(inputs: Inputs<'_>) -> Vec<String> {
    let Inputs {
        probes,
        scan_generation,
        verified,
        selected_agent,
        docker,
        host,
        git_probe,
        image_state,
    } = inputs;
    // Readiness, not merely completed probing, can resolve old tasks. A
    // stopped/rootful/untrusted Engine cannot prove an earlier prerequisite
    // was fixed. An unknown CLI account also cannot verify its old failures.
    let mut domains = Vec::new();
    if probes
        .iter()
        .any(|probe| probe.run_generation == scan_generation && probe.binary_path.is_some())
    {
        domains.push("cli:install".into());
    }
    if probes.len() == 6
        && probes.iter().all(|probe| {
            probe.run_generation == scan_generation && probe.status != ProbeStatus::Failed
        })
    {
        domains.push("cli:scan".into());
    }
    for probe in verified {
        domains.push(format!("cli:{}", probe.provider_id));
    }
    if selected_agent.is_some_and(|id| verified.iter().any(|probe| probe.provider_id == id)) {
        domains.push("cli:select".into());
    }
    if host == HostKind::Linux
        && !docker.desktop_vm
        && docker.engine_ready
        && docker.rootless == Some(true)
    {
        domains.push("docker".into());
    }
    if matches!(host, HostKind::MacOs | HostKind::Windows)
        && docker.desktop_vm
        && docker.engine_ready
        && docker.desktop_transport_verified
        && docker.desktop_vm_capacity_verified
        && docker.workspace_staging_available
        && docker.disk_available_bytes.is_some()
        && matches!(
            docker.state,
            crate::execution::docker::environment::DockerEnvironmentState::MissingApprovedImage
                | crate::execution::docker::environment::DockerEnvironmentState::Ready
        )
    {
        domains.push("docker:desktop".into());
    }
    if git_probe.executable_state == EvidenceState::Ready {
        domains.push("git:executable".into());
    }
    if git_probe.executable_state == EvidenceState::Ready
        && git_probe.identity_state == EvidenceState::Ready
    {
        domains.push("git:identity".into());
        domains.push("git".into());
    }
    domains.extend(images::checked_domains(
        docker,
        host,
        &image_state.required_profiles,
        &image_state.smoke_verified_profiles,
    ));
    domains
}
