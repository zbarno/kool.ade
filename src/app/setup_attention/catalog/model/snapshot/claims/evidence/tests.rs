use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain,
};

use super::ready_claim;

#[test]
fn computer_kvm_prerequisite_claims_require_typed_host_readiness() {
    for capability in [Capability::LinuxKernel, Capability::LinuxUserNamespaces] {
        assert!(ready_claim(
            Domain::Host,
            capability,
            Origin::HostRuntime,
            Code::LinuxHostCheckReady,
        ));
        assert!(!ready_claim(
            Domain::Host,
            capability,
            Origin::ApplicationInventory,
            Code::LinuxHostCheckReady,
        ));
        assert!(!ready_claim(
            Domain::Host,
            capability,
            Origin::HostRuntime,
            Code::LinuxProbeInaccessible,
        ));
    }
}
