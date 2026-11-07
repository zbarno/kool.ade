use super::super::super::super::evidence::{
    ComputerUseCapabilityV1 as ObservedCapability, EvidenceCode, EvidenceOrigin, EvidenceState,
    ProbeEvidenceV1, ProbeObservationV1,
};
use super::super::super::super::identifiers::{
    HostKind, PrerequisiteCapability as Capability, PrerequisiteDomain, PrerequisiteSubject,
};

pub(crate) fn computer_use_capability_evidence_is_consistent(
    item: &ProbeEvidenceV1,
    generation: u64,
) -> bool {
    let expected = match item.prerequisite.capability {
        Capability::ComputerGuestImage => {
            (ObservedCapability::GuestImageCatalogIntegrity, item.state)
        }
        Capability::ComputerOverlay => (
            ObservedCapability::FilesystemOverlaySupport,
            EvidenceState::Unknown,
        ),
        Capability::ComputerNetworkGateway => (
            ObservedCapability::FilteredNetworkGateway,
            EvidenceState::Unsupported,
        ),
        Capability::ComputerGraphicsTransport => (
            ObservedCapability::AuthenticatedGraphicsTransport,
            EvidenceState::Unsupported,
        ),
        Capability::ComputerEncryptionKeyService => (
            ObservedCapability::PersonalEncryptionKeyService,
            EvidenceState::Unsupported,
        ),
        Capability::ComputerGuestProtocol => (
            ObservedCapability::GuestProtocolCompatibility,
            EvidenceState::Unknown,
        ),
        _ => return true,
    };
    let image_probe = item.prerequisite.capability == Capability::ComputerGuestImage;
    item.scan_generation == generation
        && item.prerequisite.domain == PrerequisiteDomain::Host
        && item.prerequisite.platform == HostKind::Linux
        && item.prerequisite.subject == PrerequisiteSubject::None
        && (image_probe || item.state == expected.1)
        && item.code
            == if image_probe {
                match item.state {
                    EvidenceState::Missing => EvidenceCode::ComputerGuestImageNotPromoted,
                    EvidenceState::Unknown => EvidenceCode::ComputerGuestImageCatalogUnverified,
                    EvidenceState::Ready => EvidenceCode::ComputerGuestImageCatalogVerified,
                    _ => return false,
                }
            } else {
                EvidenceCode::ComputerUseReadinessProbeNotIntegrated
            }
        && item.origin == EvidenceOrigin::ApplicationInventory
        && item.source_generation.is_none()
        && item.observation
            == Some(ProbeObservationV1::ComputerUseCapability {
                capability: expected.0,
                readiness_probe_integrated: image_probe,
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::setup_attention::catalog::PrerequisiteId;

    fn evidence(
        capability: Capability,
        observed_capability: ObservedCapability,
        state: EvidenceState,
    ) -> ProbeEvidenceV1 {
        ProbeEvidenceV1 {
            prerequisite: PrerequisiteId {
                domain: PrerequisiteDomain::Host,
                platform: HostKind::Linux,
                capability,
                subject: PrerequisiteSubject::None,
            },
            state,
            code: if capability == Capability::ComputerGuestImage {
                EvidenceCode::ComputerGuestImageNotPromoted
            } else {
                EvidenceCode::ComputerUseReadinessProbeNotIntegrated
            },
            origin: EvidenceOrigin::ApplicationInventory,
            scan_generation: 1,
            source_generation: None,
            observed_at_unix_ms: 1,
            observation: Some(ProbeObservationV1::ComputerUseCapability {
                capability: observed_capability,
                readiness_probe_integrated: capability == Capability::ComputerGuestImage,
            }),
        }
    }

    #[test]
    fn current_build_component_inventory_never_accepts_a_positive_claim() {
        for (capability, observed, expected_state) in [
            (
                Capability::ComputerGuestImage,
                ObservedCapability::GuestImageCatalogIntegrity,
                EvidenceState::Missing,
            ),
            (
                Capability::ComputerOverlay,
                ObservedCapability::FilesystemOverlaySupport,
                EvidenceState::Unknown,
            ),
            (
                Capability::ComputerNetworkGateway,
                ObservedCapability::FilteredNetworkGateway,
                EvidenceState::Unsupported,
            ),
            (
                Capability::ComputerGraphicsTransport,
                ObservedCapability::AuthenticatedGraphicsTransport,
                EvidenceState::Unsupported,
            ),
            (
                Capability::ComputerEncryptionKeyService,
                ObservedCapability::PersonalEncryptionKeyService,
                EvidenceState::Unsupported,
            ),
            (
                Capability::ComputerGuestProtocol,
                ObservedCapability::GuestProtocolCompatibility,
                EvidenceState::Unknown,
            ),
        ] {
            let item = evidence(capability, observed, expected_state);
            assert!(computer_use_capability_evidence_is_consistent(&item, 1));

            let mut forged_ready = item;
            forged_ready.state = EvidenceState::Ready;
            if let Some(ProbeObservationV1::ComputerUseCapability {
                readiness_probe_integrated,
                ..
            }) = forged_ready.observation.as_mut()
            {
                *readiness_probe_integrated = true;
            }
            assert!(!computer_use_capability_evidence_is_consistent(
                &forged_ready,
                1
            ));
        }
    }

    #[test]
    fn verified_guest_image_readiness_requires_integrated_probe_evidence() {
        let mut item = evidence(
            Capability::ComputerGuestImage,
            ObservedCapability::GuestImageCatalogIntegrity,
            EvidenceState::Ready,
        );
        item.code = EvidenceCode::ComputerGuestImageCatalogVerified;
        if let Some(ProbeObservationV1::ComputerUseCapability {
            readiness_probe_integrated,
            ..
        }) = item.observation.as_mut()
        {
            *readiness_probe_integrated = true;
        }
        assert!(computer_use_capability_evidence_is_consistent(&item, 1));
        item.code = EvidenceCode::ComputerGuestImageCatalogUnverified;
        assert!(!computer_use_capability_evidence_is_consistent(&item, 1));
    }
}
