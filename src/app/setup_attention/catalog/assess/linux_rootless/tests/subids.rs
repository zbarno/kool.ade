use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, Finding, FindingStatus, HostKind,
    PrerequisiteCapability as Capability, ProbeObservationV1 as Observation,
};

use super::super::{ParentDisposition, SUBIDS_ID, project};
use super::fixture::{GENERATION, candidate_evidence, candidate_matrix, set_fact};

#[test]
fn existing_incomplete_ranges_do_not_offer_an_automatic_repair() {
    let mut insufficient = candidate_evidence();
    for capability in [Capability::LinuxSubuid, Capability::LinuxSubgid] {
        set_fact(
            &mut insufficient,
            capability,
            State::Unknown,
            Code::HostCapabilityNotProbed,
            Some(Observation::LinuxSubordinateIds { configured: false }),
        );
    }
    let insufficient = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &insufficient,
        true,
    );
    assert_eq!(insufficient.disposition, ParentDisposition::CandidateManual);
    assert_eq!(
        finding(&insufficient.findings, SUBIDS_ID).status,
        FindingStatus::Unverified
    );

    let mut partial = candidate_evidence();
    set_fact(
        &mut partial,
        Capability::LinuxSubgid,
        State::Missing,
        Code::LinuxHostCheckMissing,
        Some(Observation::LinuxSubordinateIds { configured: false }),
    );
    let partial = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &partial,
        true,
    );
    assert_eq!(partial.disposition, ParentDisposition::CandidateManual);
    assert_eq!(
        finding(&partial.findings, SUBIDS_ID).status,
        FindingStatus::Unverified
    );
}

fn finding<'a>(findings: &'a [Finding], id: &str) -> &'a Finding {
    findings
        .iter()
        .find(|finding| finding.id == id)
        .expect("expected projected finding")
}
