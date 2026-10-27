//! Fresh, read-only evidence for bootstrap preconditions and verification.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::app::{
    bootstrap_remediation::{
        plan::RemediationPlan,
        supervisor::{
            PreconditionReceipt, PreconditionVerifier, ReadinessVerifier, VerificationReceipt,
        },
    },
    setup_attention::{self, SetupScanResult},
};

mod mapping;
mod system;

const PACKAGE: &[&str] = &[
    "host:linux:rootless_recipe",
    "host:linux:kernel",
    "host:linux:user_namespaces",
    "host:linux:cgroups",
    "host:linux:package_manager",
];
const SUBIDS: &[&str] = &[
    "host:linux:rootless_recipe",
    "host:linux:kernel",
    "host:linux:user_namespaces",
    "host:linux:cgroups",
    "host:linux:subuid",
    "host:linux:subgid",
];
const SERVICE: &[&str] = &[
    "host:linux:rootless_recipe",
    "host:linux:kernel",
    "host:linux:user_namespaces",
    "host:linux:cgroups",
    "host:linux:rootless_helpers",
    "host:linux:service_manager",
    "host:linux:user_runtime_directory",
    "host:linux:rootless_service_absent",
    "host:linux:rootlesskit_smoke",
    "docker:endpoint_environment",
];
const HELPERS: &[&str] = &["host:linux:rootless_helpers"];
const UID_MAPPINGS: &[&str] = &[
    "host:linux:subuid",
    "host:linux:subgid",
    "host:linux:uid_mappings",
];
const SERVICE_VERIFY: &[&str] = &[
    "host:linux:rootless_recipe",
    "host:linux:rootless_helpers",
    "host:linux:service_manager",
    "docker:rootless",
    "docker:rootless-smoke",
];
const EMPTY: &[&str] = &[];
const PRECONDITIONS: &[&[&str]] = &[EMPTY, PACKAGE, SUBIDS, SERVICE];
const VERIFICATIONS: &[&[&str]] = &[&["docker:rootless"], HELPERS, UID_MAPPINGS, SERVICE_VERIFY];
static GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) struct LinuxReadinessVerifier;

impl PreconditionVerifier for LinuxReadinessVerifier {
    fn verify(&self, probe_ids: &[String]) -> anyhow::Result<PreconditionReceipt> {
        anyhow::ensure!(
            supports(probe_ids, PRECONDITIONS),
            "unsupported or incomplete precondition probe set"
        );
        let evidence = mapping::collect(&SystemSource, probe_ids, mapping::Phase::Before)?;
        Ok(PreconditionReceipt {
            probe_ids: probe_ids.to_vec(),
            observed_at_unix_ms: evidence.at,
            evidence_digest: evidence.digest,
            ready: evidence.ready,
        })
    }
}

impl ReadinessVerifier for LinuxReadinessVerifier {
    fn verify(&self, plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt> {
        let ids = &plan.verification_probe_ids;
        anyhow::ensure!(
            supports(ids, VERIFICATIONS),
            "unsupported or incomplete verification probe set"
        );
        let evidence = mapping::collect(&SystemSource, ids, mapping::Phase::After)?;
        Ok(VerificationReceipt {
            probe_ids: ids.clone(),
            observed_at_unix_ms: evidence.at,
            evidence_digest: evidence.digest,
            ready: evidence.ready || evidence.service_ready_awaiting_smoke,
        })
    }
}

trait ReadinessSource {
    fn scan(&self) -> anyhow::Result<SetupScanResult>;
    fn service_absent(&self) -> system::ProbeOutcome;
    fn rootlesskit_smoke(&self) -> system::ProbeOutcome;
    fn endpoint(&self) -> system::EndpointObservation;
}

struct SystemSource;

impl ReadinessSource for SystemSource {
    fn scan(&self) -> anyhow::Result<SetupScanResult> {
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let scan = setup_attention::scan(generation, cancel);
        anyhow::ensure!(!scan.cancelled, "fresh setup scan was cancelled");
        let snapshot = scan
            .snapshot
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("fresh setup scan lacks evidence"))?;
        anyhow::ensure!(
            scan.generation == snapshot.scan_generation,
            "setup scan generation changed"
        );
        snapshot.validate()?;
        Ok(scan)
    }

    fn service_absent(&self) -> system::ProbeOutcome {
        system::service_absent()
    }
    fn rootlesskit_smoke(&self) -> system::ProbeOutcome {
        system::rootlesskit_smoke()
    }
    fn endpoint(&self) -> system::EndpointObservation {
        system::current_endpoint()
    }
}

fn supports(ids: &[String], allowed: &[&[&str]]) -> bool {
    let unique = ids.iter().collect::<std::collections::BTreeSet<_>>();
    unique.len() == ids.len()
        && allowed
            .iter()
            .any(|set| ids.iter().map(String::as_str).eq(set.iter().copied()))
}

#[cfg(test)]
#[path = "readiness/tests.rs"]
mod tests;
