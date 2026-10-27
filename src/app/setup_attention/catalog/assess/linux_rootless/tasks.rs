use crate::app::setup_attention::catalog::{HostKind, ProbeEvidenceV1 as Evidence};

use super::{
    FindingStatus, PACKAGES_ID, ParentDisposition, Projection, SUBIDS_ID, TaskReadiness, facts,
    findings,
};

pub(super) fn project_candidate(
    host: HostKind,
    generation: u64,
    evidence: &[Evidence],
) -> Projection {
    let baseline_ready = facts::baseline_ready(evidence, host, generation);
    let package_readiness = facts::package_readiness(evidence, host, generation, baseline_ready);
    let subids_readiness = facts::subids_readiness(evidence, host, generation, baseline_ready);
    let service_readiness = facts::service_readiness(
        evidence,
        host,
        generation,
        baseline_ready,
        package_readiness,
        subids_readiness,
    );
    let mut projected = Vec::new();

    if package_readiness != TaskReadiness::NotRequired {
        projected.push(findings::package(package_readiness));
    }
    if subids_readiness != TaskReadiness::NotRequired {
        let mut prerequisites = Vec::new();
        if package_readiness != TaskReadiness::NotRequired {
            prerequisites.push(PACKAGES_ID.to_owned());
        }
        projected.push(findings::subids(subids_readiness, prerequisites));
    }

    let mut service_prerequisites = Vec::new();
    if package_readiness != TaskReadiness::NotRequired {
        service_prerequisites.push(PACKAGES_ID.to_owned());
    }
    if subids_readiness != TaskReadiness::NotRequired {
        service_prerequisites.push(SUBIDS_ID.to_owned());
    }
    projected.push(findings::service(service_readiness, service_prerequisites));

    let manual = projected
        .iter()
        .any(|finding| finding.status == FindingStatus::Unverified);
    Projection {
        disposition: if manual {
            ParentDisposition::CandidateManual
        } else {
            ParentDisposition::Candidate
        },
        findings: projected,
    }
}
