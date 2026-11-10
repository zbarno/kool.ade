use super::*;

const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn enrollment(worker_id: &str, fingerprint: &str) -> ComputerRemoteWorkerEnrollmentV1 {
    let worker_key = ed25519_dalek::SigningKey::from_bytes(&[5; 32]);
    ComputerRemoteWorkerEnrollmentV1 {
        schema_version: 1,
        enabled: true,
        worker_id: worker_id.into(),
        machine_id: "00000000-0000-4000-8000-000000000098".into(),
        ssh_host_alias: format!("{worker_id}.example.test"),
        ssh_principal: "computer-worker".into(),
        known_hosts_path: "/home/operator/.ssh/known_hosts".into(),
        ssh_binary_path: "/usr/bin/ssh".into(),
        worker_config_path: "/etc/koolade/computer-use-worker.json".into(),
        ssh_host_key_fingerprint: fingerprint.into(),
        worker_attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
        worker_attestation_verifying_key_hex: encode_hex(worker_key.verifying_key().as_bytes()),
        transport_protocol_version:
            crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        trusted_image_digests: [IMAGE.into()].into_iter().collect(),
        max_vcpus: 8,
        max_memory_bytes: 8 * 1024 * 1024 * 1024,
        max_disk_bytes: 32 * 1024 * 1024 * 1024,
        max_parallel_sessions: 2,
        enrolled_at_unix_ms: 1_800_000_000_000,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn advertisement(worker_id: &str, fingerprint: &str) -> ComputerRemoteWorkerAdvertisementV1 {
    let mut advertisement = ComputerRemoteWorkerAdvertisementV1 {
        schema_version: 1,
        worker_id: worker_id.into(),
        attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
        signature_hex: String::new(),
        ssh_host_key_fingerprint: fingerprint.into(),
        transport_protocol_version:
            crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        observed_at_unix_ms: 1_800_000_000_000,
        cpu_virtualization_available: true,
        image_digests: vec![IMAGE.into()],
        display_streaming_available: true,
        cpu_millis: 8_000,
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

fn requirements() -> ComputerRemoteWorkerRequirementsV1 {
    ComputerRemoteWorkerRequirementsV1 {
        image_digest: IMAGE.into(),
        cpu_millis: 2_000,
        memory_bytes: 4 * 1024 * 1024 * 1024,
        disk_bytes: 16 * 1024 * 1024 * 1024,
        require_display_streaming: true,
        session_class: ComputerSessionClassV1::Disposable,
    }
}

#[test]
fn two_independent_worker_fixtures_are_bound_to_their_own_enrollment() {
    // Synthetic independent-host fixtures prove identity separation only;
    // they do not claim that two physical hosts were contacted.
    let east_fp = format!("SHA256:{}", "A".repeat(43));
    let west_fp = format!("SHA256:{}", "B".repeat(43));
    let east = enrollment("worker-east", &east_fp);
    let west = enrollment("worker-west", &west_fp);

    assert_eq!(
        assess_remote_computer_worker_v1(
            Some(&east),
            &advertisement("worker-east", &east_fp),
            &requirements(),
            1_800_000_010_000,
        ),
        RemoteComputerWorkerEligibilityV1::Eligible
    );
    assert_eq!(
        assess_remote_computer_worker_v1(
            Some(&west),
            &advertisement("worker-west", &west_fp),
            &requirements(),
            1_800_000_010_000,
        ),
        RemoteComputerWorkerEligibilityV1::Eligible
    );
    assert_eq!(
        assess_remote_computer_worker_v1(
            Some(&east),
            &advertisement("worker-west", &west_fp),
            &requirements(),
            1_800_000_010_000,
        ),
        RemoteComputerWorkerEligibilityV1::ProtocolOrIdentityMismatch
    );
}

#[test]
fn enrollment_requires_a_static_safe_ssh_alias_and_principal() {
    let fingerprint = format!("SHA256:{}", "C".repeat(43));
    let mut enrolled = enrollment("worker-east", &fingerprint);
    enrolled.ssh_host_alias = "host;curl attacker".into();
    assert!(enrolled.validate().is_err());

    let mut enrolled = enrollment("worker-east", &fingerprint);
    enrolled.ssh_principal = "user@host".into();
    assert!(enrolled.validate().is_err());
}

#[test]
fn capability_observations_must_match_controller_policy_and_freshness() {
    let fingerprint = format!("SHA256:{}", "C".repeat(43));
    let enrolled = enrollment("worker-c", &fingerprint);
    let mut observed = advertisement("worker-c", &fingerprint);
    observed.image_digests.clear();
    assert_eq!(
        assess_remote_computer_worker_v1(
            Some(&enrolled),
            &observed,
            &requirements(),
            1_800_000_010_000,
        ),
        RemoteComputerWorkerEligibilityV1::InvalidAdvertisement
    );

    let mut stale = advertisement("worker-c", &fingerprint);
    stale.observed_at_unix_ms = 1_800_000_000_000;
    assert_eq!(
        assess_remote_computer_worker_v1(
            Some(&enrolled),
            &stale,
            &requirements(),
            1_800_000_060_001,
        ),
        RemoteComputerWorkerEligibilityV1::StaleAdvertisement
    );

    let mut not_enrolled_image = requirements();
    not_enrolled_image.image_digest = format!("sha256:{}", "d".repeat(64));
    assert_eq!(
        assess_remote_computer_worker_v1(
            Some(&enrolled),
            &advertisement("worker-c", &fingerprint),
            &not_enrolled_image,
            1_800_000_010_000,
        ),
        RemoteComputerWorkerEligibilityV1::ImageNotApproved
    );
}

#[test]
fn worker_reported_encryption_does_not_enable_personal_persistent_vms() {
    let fingerprint = format!("SHA256:{}", "D".repeat(43));
    let enrolled = enrollment("worker-d", &fingerprint);
    let mut requirements = requirements();
    requirements.session_class = ComputerSessionClassV1::PersonalPersistent;
    let mut observed = advertisement("worker-d", &fingerprint);
    observed.encrypted_personal_storage_available = true;

    assert_eq!(
        assess_remote_computer_worker_v1(
            Some(&enrolled),
            &observed,
            &requirements,
            1_800_000_010_000,
        ),
        RemoteComputerWorkerEligibilityV1::PersonalVmNotInScope
    );
}
