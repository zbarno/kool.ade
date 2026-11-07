use super::*;

fn storage_explanation(resources: super::super::resources::ResourceCapacity) -> String {
    let cards = super::build_with_guest_image_state(
        image_facts(resources),
        super::super::guest_image::probe().state,
    );
    cards
        .into_iter()
        .find(|card| card.id == "setup:computer:storage")
        .unwrap()
        .explanation
}

fn image_facts(resources: super::super::resources::ResourceCapacity) -> VmFacts {
    VmFacts {
        architecture: Check::Ready(()),
        distribution: Check::Ready(()),
        kernel: Check::Unknown,
        user_namespaces: Check::Unknown,
        cpu_virtualization: Check::Unknown,
        kvm: Check::Unknown,
        qemu: Check::Unknown,
        qemu_launcher_containment: Check::Ready(()),
        resources,
        nested_virtualization: Check::Unknown,
    }
}

#[test]
fn storage_card_only_claims_measurements_that_are_available() {
    let storage = storage_explanation(Default::default());
    assert!(storage.contains("could not both be measured"));

    let space_only = storage_explanation(super::super::resources::ResourceCapacity {
        available_storage_bytes: Some(8 * 1024 * 1024 * 1024),
        ..Default::default()
    });
    assert!(space_only.contains("Free storage was measured"));

    let filesystem = storage_explanation(super::super::resources::ResourceCapacity {
        available_storage_bytes: Some(8 * 1024 * 1024 * 1024),
        filesystem_family: Some(
            crate::app::setup_attention::catalog::LinuxVmStorageFilesystemV1::Ext,
        ),
        ..Default::default()
    });
    assert!(filesystem.contains("Free storage and its filesystem family were measured"));
    assert!(filesystem.contains("not verified"));
}

#[test]
fn image_card_tracks_signed_cache_readiness_and_disappears_when_ready() {
    let ready = super::build_with_guest_image_state(
        image_facts(Default::default()),
        crate::app::setup_attention::catalog::EvidenceState::Ready,
    );
    assert!(
        ready
            .iter()
            .all(|card| card.id != "setup:computer:image:amd64")
    );

    let missing = super::build_with_guest_image_state(
        image_facts(Default::default()),
        crate::app::setup_attention::catalog::EvidenceState::Missing,
    );
    let image = missing
        .iter()
        .find(|card| card.id == "setup:computer:image:amd64")
        .unwrap();
    assert_eq!(image.status, FindingStatus::Unsupported);
    assert!(image.explanation.contains("no promoted"));

    let unverified = super::build_with_guest_image_state(
        image_facts(Default::default()),
        crate::app::setup_attention::catalog::EvidenceState::Unknown,
    );
    let image = unverified
        .iter()
        .find(|card| card.id == "setup:computer:image:amd64")
        .unwrap();
    assert_eq!(image.status, FindingStatus::Unverified);
    assert!(image.explanation.contains("anti-rollback"));
}

#[test]
fn verified_image_is_independent_of_host_distro_and_nested_virtualization() {
    let mut facts = image_facts(Default::default());
    facts.distribution = Check::Unsupported;
    facts.nested_virtualization = Check::Unsupported;
    let cards = super::build_with_guest_image_state(
        facts,
        crate::app::setup_attention::catalog::EvidenceState::Ready,
    );
    assert!(
        cards
            .iter()
            .all(|card| card.id != "setup:computer:image:amd64")
    );
}
