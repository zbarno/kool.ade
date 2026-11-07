use super::*;
use crate::app::setup_attention::catalog::{
    ComputerUseCapabilityV1, EvidenceCode, EvidenceOrigin, EvidenceState, HostKind,
    PrerequisiteCapability, PrerequisiteDomain, PrerequisiteId, PrerequisiteSubject,
    ProbeEvidenceV1, ProbeObservationV1,
};

fn verified_guest_image_fact(generation: u64, at: u64) -> ProbeEvidenceV1 {
    ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: HostKind::Linux,
            capability: PrerequisiteCapability::ComputerGuestImage,
            subject: PrerequisiteSubject::None,
        },
        state: EvidenceState::Ready,
        code: EvidenceCode::ComputerGuestImageCatalogVerified,
        origin: EvidenceOrigin::ApplicationInventory,
        scan_generation: generation,
        source_generation: None,
        observed_at_unix_ms: at,
        observation: Some(ProbeObservationV1::ComputerUseCapability {
            capability: ComputerUseCapabilityV1::GuestImageCatalogIntegrity,
            readiness_probe_integrated: true,
        }),
    }
}

/// Production boundary: machine-scan reconciliation converts setup findings
/// and checked domains into durable task verification state. This guards the
/// scan-to-journal path against a provider-supplied computer domain; a store
/// validator unit test alone would not prove that reconciliation rejects it.
/// External dependencies: none; the store is in memory. Setup cost: negligible.
#[test]
fn provider_claimed_computer_readiness_cannot_verify_a_vm_task() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[("setup:computer:qemu", FindingStatus::Unverified, &[])],
            &["computer:qemu"],
            10,
        ),
    )
    .unwrap();
    let task = &store.tasks["setup:computer:qemu"];
    assert_eq!(task.state, SetupTaskStateV1::NeedsVerification);
    assert!(task.verified_result.is_none());

    let mut forged = store.clone();
    let task = forged.tasks.get_mut("setup:computer:qemu").unwrap();
    task.state = SetupTaskStateV1::Verified;
    task.verified_at_unix_ms = Some(10);
    task.verified_result = Some(super::super::super::VerifiedResultV1 {
        evidence_domain: "computer:qemu".into(),
        scan_generation: 1,
        verified_at_unix_ms: 10,
    });
    assert!(forged.validate().is_err());
}

/// Production boundary: reopen and rescan the actual Computer Use task IDs
/// through journal reconciliation. A unit test of task ID construction alone
/// cannot prove serialized state keeps stable IDs without duplicate cards.
/// External dependencies: none; setup cost is negligible.
#[test]
fn reopened_computer_tasks_rescan_without_duplicates_or_losing_dependencies() {
    let findings = [
        (
            "setup:computer:image:amd64",
            FindingStatus::Unverified,
            &[][..],
        ),
        (
            "setup:computer:qemu",
            FindingStatus::NeedsAttention,
            &[][..],
        ),
        (
            "setup:computer:storage",
            FindingStatus::Unverified,
            &["setup:computer:image:amd64"][..],
        ),
        (
            "setup:computer:viewer",
            FindingStatus::Unverified,
            &[
                "setup:computer:image:amd64",
                "setup:computer:qemu",
                "setup:computer:storage",
            ][..],
        ),
    ];
    let mut store = SetupTaskStoreV1::default();
    scan(&mut store, &snapshot(&findings, &[], 10)).unwrap();

    let serialized = serde_json::to_vec(&store).unwrap();
    let mut reopened: SetupTaskStoreV1 = serde_json::from_slice(&serialized).unwrap();
    scan(&mut reopened, &snapshot(&findings, &[], 20)).unwrap();

    assert_eq!(reopened.tasks.len(), findings.len());
    assert_eq!(
        reopened
            .tasks
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "setup:computer:image:amd64",
            "setup:computer:qemu",
            "setup:computer:storage",
            "setup:computer:viewer"
        ]
    );
    assert_eq!(
        reopened.tasks["setup:computer:viewer"]
            .dependencies
            .iter()
            .map(|dependency| dependency.prerequisite_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "setup:computer:image:amd64",
            "setup:computer:qemu",
            "setup:computer:storage"
        ]
    );
}

/// Production boundary: scan reconciliation converts a verified guest-image
/// probe into a current dependency receipt, including when the blocker card
/// disappears. A graph-unit test cannot establish this journal transition.
/// External dependencies: none; in-memory setup store. Setup cost: negligible.
#[test]
fn verified_guest_image_unblocks_existing_tasks_and_new_dependencies() {
    let image_id = "setup:computer:image:amd64";
    let storage_id = "setup:computer:storage";
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[
                (image_id, FindingStatus::Unsupported, &[]),
                (storage_id, FindingStatus::Unverified, &[image_id]),
            ],
            &[],
            10,
        ),
    )
    .unwrap();
    assert_eq!(
        store.tasks[storage_id].dependencies[0].resolution,
        DependencyResolution::Pending
    );

    let mut ready = snapshot(
        &[(storage_id, FindingStatus::Unverified, &[image_id])],
        &["computer:guest-image"],
        20,
    );
    ready.probe_evidence.push(verified_guest_image_fact(
        store.latest_scan_generation + 1,
        20,
    ));
    scan(&mut store, &ready).unwrap();
    assert_eq!(store.tasks[image_id].state, SetupTaskStateV1::Verified);
    assert_eq!(
        store.tasks[image_id]
            .verified_result
            .as_ref()
            .unwrap()
            .evidence_domain,
        "computer:guest-image"
    );
    assert_eq!(
        store.tasks[storage_id].dependencies[0].resolution,
        DependencyResolution::SatisfiedByTask { scan_generation: 2 }
    );

    let mut no_prior_card = SetupTaskStoreV1::default();
    let mut first_scan = snapshot(
        &[(storage_id, FindingStatus::Unverified, &[image_id])],
        &["computer:guest-image"],
        30,
    );
    first_scan
        .probe_evidence
        .push(verified_guest_image_fact(1, 30));
    scan(&mut no_prior_card, &first_scan).unwrap();
    assert_eq!(
        no_prior_card.tasks[storage_id].dependencies[0].resolution,
        DependencyResolution::SatisfiedByProbe {
            evidence_domain: "computer:guest-image".into(),
            scan_generation: 1,
            verified_at_unix_ms: 30,
        }
    );
}
