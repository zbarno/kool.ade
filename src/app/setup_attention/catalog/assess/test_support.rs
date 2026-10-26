use super::*;

pub fn assess(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
) -> MachineSnapshotV1 {
    assess_for_scan(probes, selected_agent, docker, host, checked_at_unix_ms, 0)
}

pub fn assess_for_scan(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
    scan_generation: u64,
) -> MachineSnapshotV1 {
    assess_for_scan_with_git(
        probes,
        selected_agent,
        docker,
        host,
        checked_at_unix_ms,
        scan_generation,
        GitProbeV1::not_run(),
    )
}

pub fn assess_for_scan_with_git(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
    scan_generation: u64,
    git_probe: GitProbeV1,
) -> MachineSnapshotV1 {
    assess_for_scan_with_git_and_bootstrap_provider(
        probes,
        ProviderChoices {
            default_agent: selected_agent,
            guided_setup: None,
        },
        docker,
        host,
        checked_at_unix_ms,
        scan_generation,
        git_probe,
    )
}
