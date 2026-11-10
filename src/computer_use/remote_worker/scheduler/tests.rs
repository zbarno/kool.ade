use super::*;
use crate::computer_use::{
    ComputerSessionClassV1, remote_worker::ComputerRemoteWorkerEnrollmentV1 as Enrollment,
    remote_worker::ComputerRemoteWorkerRequirementsV1 as Requirements,
    remote_worker::RemoteComputerWorkerEligibilityV1 as Eligibility,
};

const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NOW: u64 = 1_800_000_010_000;

fn fingerprint() -> String {
    format!("SHA256:{}", "A".repeat(43))
}

fn enrollment() -> Enrollment {
    let worker_key = ed25519_dalek::SigningKey::from_bytes(&[5; 32]);
    Enrollment {
        schema_version: 1,
        enabled: true,
        worker_id: "worker-east".into(),
        machine_id: "00000000-0000-4000-8000-000000000098".into(),
        ssh_host_alias: "worker-east.invalid".into(),
        ssh_principal: "computer-worker".into(),
        known_hosts_path: "/etc/ssh/ssh_known_hosts".into(),
        ssh_binary_path: "/usr/bin/ssh".into(),
        worker_config_path: "/etc/koolade/computer-use-worker.json".into(),
        ssh_host_key_fingerprint: fingerprint(),
        worker_attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
        worker_attestation_verifying_key_hex: worker_key
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        transport_protocol_version:
            crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        enrolled_at_unix_ms: NOW - 100_000,
        trusted_image_digests: [IMAGE.into()].into_iter().collect(),
        max_vcpus: 4,
        max_memory_bytes: 8 * 1024 * 1024 * 1024,
        max_disk_bytes: 24 * 1024 * 1024 * 1024,
        max_parallel_sessions: 2,
    }
}

fn advertisement() -> ComputerRemoteWorkerAdvertisementV1 {
    let mut advertisement = ComputerRemoteWorkerAdvertisementV1 {
        schema_version: 1,
        worker_id: "worker-east".into(),
        attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
        signature_hex: String::new(),
        ssh_host_key_fingerprint: fingerprint(),
        transport_protocol_version:
            crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        observed_at_unix_ms: NOW,
        cpu_virtualization_available: true,
        image_digests: vec![IMAGE.into()],
        display_streaming_available: true,
        cpu_millis: 4_000,
        memory_bytes: 8 * 1024 * 1024 * 1024,
        disk_bytes: 24 * 1024 * 1024 * 1024,
        encrypted_personal_storage_available: false,
        parallel_slots: 2,
    };
    advertisement
        .attest(
            "00000000-0000-4000-8000-000000000009".into(),
            &ed25519_dalek::SigningKey::from_bytes(&[5; 32]),
        )
        .unwrap();
    advertisement
}

fn requirements() -> Requirements {
    Requirements {
        image_digest: IMAGE.into(),
        cpu_millis: 2_000,
        memory_bytes: 4 * 1024 * 1024 * 1024,
        disk_bytes: 12 * 1024 * 1024 * 1024,
        require_display_streaming: true,
        session_class: ComputerSessionClassV1::Disposable,
    }
}

#[test]
fn production_scheduler_stays_closed_until_the_real_worker_release_gate() {
    assert!(!crate::computer_use::remote_worker::remote_computer_use_release_gate_open_v1());
    let scheduler = ComputerRemoteWorkerSchedulerV1::new(8, 16).unwrap();
    let result = scheduler.schedule_approved(
        &enrollment(),
        &advertisement(),
        &requirements(),
        "worker-east",
        "session-a",
        NOW,
    );
    assert_eq!(result.unwrap_err(), Eligibility::ReleaseGateClosed);
    assert_eq!(scheduler.active_sessions(), 0);
}

#[test]
fn explicit_approval_is_required_and_no_local_or_other_worker_fallback_occurs() {
    let scheduler = ComputerRemoteWorkerSchedulerV1::new(8, 16).unwrap();
    assert_eq!(
        scheduler
            .schedule_with_release_gate(
                &enrollment(),
                &advertisement(),
                &requirements(),
                "worker-west",
                "session-a",
                NOW,
                true,
            )
            .unwrap_err(),
        Eligibility::TargetApprovalMismatch
    );
    assert_eq!(scheduler.active_sessions(), 0);
}

#[test]
fn bounded_reservations_cover_slots_cpu_memory_disk_and_release_on_drop() {
    let scheduler = ComputerRemoteWorkerSchedulerV1::new(4, 3).unwrap();
    let first = scheduler
        .schedule_with_release_gate(
            &enrollment(),
            &advertisement(),
            &requirements(),
            "worker-east",
            "session-a",
            NOW,
            true,
        )
        .unwrap();
    let second = scheduler
        .schedule_with_release_gate(
            &enrollment(),
            &advertisement(),
            &requirements(),
            "worker-east",
            "session-b",
            NOW,
            true,
        )
        .unwrap();
    assert_eq!(first.worker_id, "worker-east");
    assert_eq!(scheduler.active_sessions(), 2);
    assert_eq!(
        scheduler
            .schedule_with_release_gate(
                &enrollment(),
                &advertisement(),
                &requirements(),
                "worker-east",
                "session-c",
                NOW,
                true,
            )
            .unwrap_err(),
        Eligibility::CapacityUnavailable
    );
    drop(first);
    assert_eq!(scheduler.active_sessions(), 1);
    let third = scheduler
        .schedule_with_release_gate(
            &enrollment(),
            &advertisement(),
            &requirements(),
            "worker-east",
            "session-c",
            NOW,
            true,
        )
        .unwrap();
    assert_eq!(scheduler.active_sessions(), 2);
    drop((second, third));
    assert_eq!(scheduler.active_sessions(), 0);
}
