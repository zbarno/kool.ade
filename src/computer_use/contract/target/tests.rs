use super::*;

fn trust() -> ComputerRemoteWorkerTrustV1 {
    ComputerRemoteWorkerTrustV1 {
        ssh_host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
        worker_attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
        worker_attestation_verifying_key_hex: encode_hex(
            &ed25519_dalek::SigningKey::from_bytes(&[9; 32])
                .verifying_key()
                .to_bytes(),
        ),
        transport_protocol_version: COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn remote_vm_target_binds_enrolled_identity_and_protocol_versions() {
    let target = ComputerTargetV1::RemoteVmWorker {
        worker_id: "worker-east".into(),
        machine_id: "00000000-0000-4000-8000-000000000098".into(),
        trust: trust(),
    };

    assert_eq!(target.validate(), Ok(()));
    assert!(target.is_remote_vm_worker());
    assert_eq!(target.worker_id(), "worker-east");
    assert_eq!(target.local_machine_id(), None);
    let value = serde_json::to_value(&target).unwrap();
    let decoded: ComputerTargetV1 = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, target);
}

#[test]
fn remote_vm_target_rejects_unpinned_or_incompatible_worker_identity() {
    let mut changed_host_key = trust();
    changed_host_key.ssh_host_key_fingerprint = "SHA256:untrusted".into();
    assert!(changed_host_key.validate().is_err());

    let mut changed_transport = trust();
    changed_transport.transport_protocol_version += 1;
    assert!(changed_transport.validate().is_err());

    let mut changed_guest = trust();
    changed_guest.guest_protocol_version += 1;
    assert!(changed_guest.validate().is_err());

    for worker_id in [
        String::new(),
        "-worker".into(),
        "worker/host".into(),
        "x".repeat(129),
    ] {
        let target = ComputerTargetV1::RemoteVmWorker {
            worker_id: worker_id.clone(),
            machine_id: "00000000-0000-4000-8000-000000000098".into(),
            trust: trust(),
        };
        assert!(
            target.validate().is_err(),
            "accepted worker ID {worker_id:?}"
        );
    }
}

#[test]
fn local_vm_target_names_hypervisor_without_claiming_other_backends() {
    let target = ComputerTargetV1::LocalVm {
        machine_id: "00000000-0000-4000-8000-000000000099".into(),
        hypervisor: ComputerHypervisorV1::QemuKvm,
    };

    assert_eq!(target.validate(), Ok(()));
    assert_eq!(
        target.local_machine_id(),
        Some("00000000-0000-4000-8000-000000000099")
    );
    assert!(!target.is_remote_vm_worker());
}
