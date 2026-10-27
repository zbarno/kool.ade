use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, HostKind, ProbeObservationV1 as Observation,
};

use super::super::{ParentDisposition, project};
use super::fixture::{GENERATION, candidate_evidence, candidate_matrix, set_fact};

#[test]
fn kernel_compatibility_floor_is_required_for_automatic_repairs() {
    let mut evidence = candidate_evidence();
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxKernel,
        State::Ready,
        Code::LinuxHostCheckReady,
        Some(Observation::LinuxKernel {
            version: crate::app::setup_attention::catalog::LinuxKernelVersionV1 {
                major: 4,
                minor: 14,
                patch: 0,
            },
        }),
    );
    let below_floor = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &evidence,
        true,
    );
    assert_eq!(below_floor.disposition, ParentDisposition::CandidateManual);

    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxKernel,
        State::Ready,
        Code::LinuxHostCheckReady,
        Some(Observation::LinuxKernel {
            version: crate::app::setup_attention::catalog::LinuxKernelVersionV1 {
                major: 4,
                minor: 15,
                patch: 0,
            },
        }),
    );
    let at_floor = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &evidence,
        true,
    );
    assert_eq!(at_floor.disposition, ParentDisposition::Candidate);
}
