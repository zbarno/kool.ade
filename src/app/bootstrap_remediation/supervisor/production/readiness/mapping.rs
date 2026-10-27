use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::app::setup_attention::{SetupScanResult, catalog as setup};

use super::ReadinessSource;

mod host;
pub(super) use host::host_ready;

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Before,
    After,
}

pub(super) struct ReceiptEvidence {
    pub ready: bool,
    pub service_ready_awaiting_smoke: bool,
    pub at: u64,
    pub digest: String,
}

#[derive(Serialize)]
struct Binding<'a> {
    generation: u64,
    checked_at: u64,
    observed_at: u64,
    ids: &'a [String],
    endpoint: &'a Option<String>,
    daemon: &'a Option<String>,
    docker_readiness: &'a Option<String>,
    values: Vec<serde_json::Value>,
}

pub(super) fn collect(
    source: &impl ReadinessSource,
    ids: &[String],
    phase: Phase,
) -> anyhow::Result<ReceiptEvidence> {
    let scan = source.scan()?;
    let snapshot = scan
        .snapshot
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("setup scan lacks snapshot"))?;
    anyhow::ensure!(
        snapshot.host == setup::HostKind::Linux,
        "Linux evidence is required"
    );
    let mut values = Vec::with_capacity(ids.len());
    let mut ready = true;
    let mut ready_without_smoke = true;
    let mut smoke_requested = false;
    let mut smoke_ready = false;
    let mut latest = snapshot.checked_at_unix_ms;
    for id in ids {
        let (satisfied, value, observed_at) = map_probe(source, &scan, id, phase)?;
        ready &= satisfied;
        if id == "docker:rootless-smoke" {
            smoke_requested = true;
            smoke_ready = satisfied;
        } else {
            ready_without_smoke &= satisfied;
        }
        latest = latest.max(observed_at);
        values.push(serde_json::json!({
            "probeId": id,
            "observedAtUnixMs": observed_at,
            "evidence": value,
        }));
    }
    let at = timestamp_after(latest)?;
    let binding = Binding {
        generation: snapshot.scan_generation,
        checked_at: snapshot.checked_at_unix_ms,
        observed_at: at,
        ids,
        endpoint: &scan.endpoint_fingerprint,
        daemon: &scan.daemon_identity_fingerprint,
        docker_readiness: &scan.docker_readiness_fingerprint,
        values,
    };
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&binding)?));
    let service_ready_awaiting_smoke = smoke_requested
        && !smoke_ready
        && ready_without_smoke
        && ids
            .iter()
            .map(String::as_str)
            .eq(super::SERVICE_VERIFY.iter().copied());
    Ok(ReceiptEvidence {
        ready,
        service_ready_awaiting_smoke,
        at,
        digest,
    })
}

fn map_probe(
    source: &impl ReadinessSource,
    scan: &SetupScanResult,
    id: &str,
    phase: Phase,
) -> anyhow::Result<(bool, serde_json::Value, u64)> {
    let snapshot = scan.snapshot.as_ref().expect("fresh scan validated");
    let result = match (id, phase) {
        ("host:linux:rootless_service_absent", Phase::Before) => {
            let item = source.service_absent();
            (item.ready(), serde_json::to_value(item)?, unix_ms()?)
        }
        ("host:linux:rootlesskit_smoke", Phase::Before) => {
            let item = source.rootlesskit_smoke();
            (item.ready(), serde_json::to_value(item)?, unix_ms()?)
        }
        ("docker:endpoint_environment", Phase::Before) => {
            let current = source.endpoint();
            let stable = scan.endpoint_fingerprint.is_some()
                && current.fingerprint == scan.endpoint_fingerprint;
            (
                current.trusted && stable,
                serde_json::to_value((current, stable))?,
                unix_ms()?,
            )
        }
        ("docker:rootless", Phase::After) => {
            let rootless =
                docker_evidence(snapshot, setup::PrerequisiteCapability::DockerRootless)?;
            let engine = docker_evidence(snapshot, setup::PrerequisiteCapability::DockerEngine)?;
            let endpoint =
                docker_evidence(snapshot, setup::PrerequisiteCapability::DockerEndpoint)?;
            let current = source.endpoint();
            let stable = scan.endpoint_fingerprint.is_some()
                && current.fingerprint == scan.endpoint_fingerprint;
            let hashes = [
                scan.endpoint_fingerprint.as_deref(),
                scan.daemon_identity_fingerprint.as_deref(),
                scan.docker_readiness_fingerprint.as_deref(),
            ]
            .into_iter()
            .all(|value| value.is_some_and(is_sha256));
            let ok = [rootless, engine, endpoint].into_iter().all(docker_ready)
                && current.trusted
                && stable
                && hashes;
            let value = serde_json::json!({"rootless":rootless,"engine":engine,"endpoint":endpoint,
                "current":current,"stable":stable,"fingerprints":[scan.endpoint_fingerprint.as_deref(),
                scan.daemon_identity_fingerprint.as_deref(),scan.docker_readiness_fingerprint.as_deref()]});
            (ok, value, unix_ms()?)
        }
        ("docker:rootless-smoke", Phase::After) => {
            let checked = snapshot
                .checked_domains
                .iter()
                .any(|domain| domain == "docker:rootless-smoke");
            let evidence = snapshot.probe_evidence.iter().find(|item| {
                item.prerequisite.domain == setup::PrerequisiteDomain::Docker
                    && item.prerequisite.platform == setup::HostKind::Linux
                    && item.prerequisite.capability
                        == setup::PrerequisiteCapability::DockerRootlessSmoke
                    && matches!(
                        &item.prerequisite.subject,
                        setup::PrerequisiteSubject::Image {
                            profile: setup::ImageProfileV1::General,
                            architecture: setup::ImageArchitectureV1::Amd64
                                | setup::ImageArchitectureV1::Arm64,
                        }
                    )
                    && item.state == setup::EvidenceState::Ready
                    && item.code == setup::EvidenceCode::ApprovedImageSmokeSucceeded
                    && item.origin == setup::EvidenceOrigin::ApprovedImageSmokeTest
                    && item.scan_generation == snapshot.scan_generation
                    && item.source_generation.is_none()
            });
            let observed_at = evidence
                .map(|item| item.observed_at_unix_ms)
                .unwrap_or(snapshot.checked_at_unix_ms);
            let satisfied = checked && evidence.is_some();
            (
                satisfied,
                serde_json::json!({"checkedDomain":checked,"evidence":evidence}),
                observed_at,
            )
        }
        (host_id, _) => {
            let (domain, capability) = host_identity(host_id)?;
            let evidence = find_evidence(snapshot, domain, capability)?;
            (
                host_ready(host_id, evidence, phase),
                serde_json::to_value(evidence)?,
                evidence.observed_at_unix_ms,
            )
        }
    };
    Ok(result)
}

fn host_identity(
    id: &str,
) -> anyhow::Result<(setup::PrerequisiteDomain, setup::PrerequisiteCapability)> {
    use setup::{PrerequisiteCapability as C, PrerequisiteDomain as D};
    let capability = match id {
        "host:linux:package_manager" => C::LinuxPackageManager,
        "host:linux:rootless_recipe" => C::LinuxRootlessRecipe,
        "host:linux:kernel" => C::LinuxKernel,
        "host:linux:user_namespaces" => C::LinuxUserNamespaces,
        "host:linux:cgroups" => C::LinuxCgroups,
        "host:linux:rootless_helpers" => C::LinuxRootlessHelpers,
        "host:linux:service_manager" | "host:linux:user_runtime_directory" => {
            C::LinuxServiceManager
        }
        "host:linux:subuid" => C::LinuxSubuid,
        "host:linux:subgid" => C::LinuxSubgid,
        "host:linux:uid_mappings" => C::LinuxUidMappings,
        _ => anyhow::bail!("unknown readiness probe ID"),
    };
    Ok((D::Host, capability))
}

fn find_evidence(
    snapshot: &setup::MachineSnapshotV1,
    domain: setup::PrerequisiteDomain,
    capability: setup::PrerequisiteCapability,
) -> anyhow::Result<&setup::ProbeEvidenceV1> {
    let mut matches = snapshot.probe_evidence.iter().filter(|item| {
        item.prerequisite.domain == domain
            && item.prerequisite.platform == snapshot.host
            && item.prerequisite.capability == capability
            && item.prerequisite.subject == setup::PrerequisiteSubject::None
    });
    let item = matches
        .next()
        .ok_or_else(|| anyhow::anyhow!("snapshot lacks requested evidence"))?;
    anyhow::ensure!(
        matches.next().is_none(),
        "snapshot duplicates requested evidence"
    );
    anyhow::ensure!(
        item.scan_generation == snapshot.scan_generation && item.source_generation.is_none(),
        "readiness evidence is stale or unexpected"
    );
    Ok(item)
}

fn docker_evidence(
    snapshot: &setup::MachineSnapshotV1,
    capability: setup::PrerequisiteCapability,
) -> anyhow::Result<&setup::ProbeEvidenceV1> {
    find_evidence(snapshot, setup::PrerequisiteDomain::Docker, capability)
}

fn docker_ready(item: &setup::ProbeEvidenceV1) -> bool {
    item.state == setup::EvidenceState::Ready
        && item.code == setup::EvidenceCode::DockerReady
        && item.origin == setup::EvidenceOrigin::DockerDiscovery
}

fn timestamp_after(floor: u64) -> anyhow::Result<u64> {
    for _ in 0..500 {
        let now = unix_ms()?;
        if now > floor {
            return Ok(now);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    anyhow::bail!("readiness clock did not advance after the fresh scan")
}

fn unix_ms() -> anyhow::Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64)
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "mapping/tests.rs"]
mod tests;
