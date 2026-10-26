use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, HostCapabilityMatrixV1, HostKind,
    PrerequisiteCapability, PrerequisiteDomain, ProbeEvidenceV1 as Evidence,
};
use crate::execution::docker::{environment::DockerEnvironmentStatus, images::RuntimeProfile};

use super::super::{Finding, FindingStatus, ImageScanState};

mod facts;
mod findings;
mod identity;
mod tasks;
#[cfg(test)]
#[path = "linux_rootless/tests.rs"]
mod tests;

pub(super) const ROOTLESS_PARENT_ID: &str = "setup:docker:rootless";
pub(super) const PACKAGES_ID: &str = "setup:linux:rootless:packages";
pub(super) const SUBIDS_ID: &str = "setup:linux:rootless:subids";
pub(super) const SERVICE_ID: &str = "setup:linux:rootless:service";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParentDisposition {
    NotApplicable,
    Candidate,
    CandidateManual,
    Unsupported,
    Unknown,
}

pub(super) struct Projection {
    pub disposition: ParentDisposition,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskReadiness {
    NotRequired,
    Actionable,
    Manual,
}

pub(super) fn effective_image_scan_state(
    image_state: &ImageScanState,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
) -> ImageScanState {
    let mut state = image_state.clone();
    if host != HostKind::Linux
        || !docker.engine_ready
        || docker.desktop_vm
        || docker.rootless != Some(true)
    {
        return state;
    }
    let current_smoke = docker
        .architecture
        .as_deref()
        .filter(|architecture| matches!(*architecture, "amd64" | "arm64"))
        .is_some_and(|architecture| {
            super::super::images::general_smoke_verified(docker, architecture)
        });
    if current_smoke {
        state
            .smoke_verified_profiles
            .insert(RuntimeProfile::General);
    } else {
        state
            .smoke_verified_profiles
            .remove(&RuntimeProfile::General);
    }
    state
}

pub(super) fn project(
    host: HostKind,
    generation: u64,
    matrix: &HostCapabilityMatrixV1,
    evidence: &[Evidence],
    parent_present: bool,
) -> Projection {
    if host != HostKind::Linux || !parent_present {
        return Projection {
            disposition: ParentDisposition::NotApplicable,
            findings: Vec::new(),
        };
    }
    match identity::recipe_identity(host, generation, matrix, evidence) {
        identity::RecipeIdentity::Candidate => tasks::project_candidate(host, generation, evidence),
        identity::RecipeIdentity::Unsupported => Projection {
            disposition: ParentDisposition::Unsupported,
            findings: Vec::new(),
        },
        identity::RecipeIdentity::Unknown => Projection {
            disposition: ParentDisposition::Unknown,
            findings: Vec::new(),
        },
    }
}

pub(super) fn append_findings(
    findings: &mut Vec<Finding>,
    host: HostKind,
    generation: u64,
    matrix: &HostCapabilityMatrixV1,
    evidence: &[Evidence],
) {
    let parent_present = findings
        .iter()
        .any(|finding| finding.id == ROOTLESS_PARENT_ID);
    let projection = project(host, generation, matrix, evidence, parent_present);
    if let Some(parent) = findings
        .iter_mut()
        .find(|finding| finding.id == ROOTLESS_PARENT_ID)
    {
        update_parent(parent, projection.disposition);
    }
    findings.extend(projection.findings);
}

pub(super) fn smoke_verified(host: HostKind, generation: u64, evidence: &[Evidence]) -> bool {
    host == HostKind::Linux
        && evidence.iter().any(|item| {
            item.prerequisite.domain == PrerequisiteDomain::Docker
                && item.prerequisite.capability == PrerequisiteCapability::DockerRootlessSmoke
                && item.state == EvidenceState::Ready
                && item.code == EvidenceCode::ApprovedImageSmokeSucceeded
                && item.origin == EvidenceOrigin::ApprovedImageSmokeTest
                && item.scan_generation == generation
                && item.source_generation.is_none()
        })
}

pub(super) fn update_parent(finding: &mut Finding, disposition: ParentDisposition) {
    match disposition {
        ParentDisposition::Candidate | ParentDisposition::NotApplicable => {}
        ParentDisposition::CandidateManual => {
            finding.status = FindingStatus::Unverified;
            finding.title = "Verify Linux rootless prerequisites".into();
            finding.explanation =
                "The supported Linux distribution was identified, but one or more rootless prerequisites could not be verified.".into();
            finding.next_step =
                "Review the manual Linux prerequisite findings, then rerun the setup scan before applying changes.".into();
        }
        ParentDisposition::Unsupported => {
            finding.status = FindingStatus::Unsupported;
            finding.title = "Rootless Docker setup is not validated for this distribution".into();
            finding.explanation =
                "Kool.ad/e has no validated native rootless bootstrap recipe for the observed Linux distribution and architecture.".into();
            finding.next_step =
                "Use the distribution's official rootless Docker instructions and verify the result manually.".into();
        }
        ParentDisposition::Unknown => {
            finding.status = FindingStatus::Unverified;
            finding.title = "Verify the Linux distribution before rootless setup".into();
            finding.explanation =
                "Kool.ad/e could not confirm the exact Linux distribution, version, and architecture required by its rootless recipe.".into();
            finding.next_step =
                "Check the operating system identity and retry the read-only setup scan; host changes remain manual until it is verified.".into();
        }
    }
}
