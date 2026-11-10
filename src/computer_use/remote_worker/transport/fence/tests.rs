use ed25519_dalek::SigningKey;

use super::super::protocol::{
    ComputerRemoteWorkerRequestBodyV1 as Body, ComputerRemoteWorkerRequestV1,
};
use super::*;
use crate::computer_use::{
    ComputerActionActorV1, ComputerCapabilityV1, ComputerIdentityV1, ComputerLeaseV1,
    ComputerLifecycleV1, ComputerNetworkProfileV1, ComputerOperationV1, ComputerPolicyV1,
    ComputerResourceBudgetV1, ComputerSessionClassV1, ComputerTargetV1,
};

const NOW: u64 = 10_000;
const WORKER_ID: &str = "cu-worker-east";
const KEY_ID: &str = "00000000-0000-4000-8000-000000000008";

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn fingerprint() -> String {
    format!("SHA256:{}", "A".repeat(43))
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn enrollment() -> ComputerRemoteWorkerHostConfigV1 {
    ComputerRemoteWorkerHostConfigV1 {
        schema_version: 1,
        enabled: true,
        worker_id: WORKER_ID.into(),
        machine_id: "00000000-0000-4000-8000-000000000098".into(),
        ssh_host_key_fingerprint: fingerprint(),
        worker_attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
        worker_attestation_private_key_hex: encode_hex(&[9; 32]),
        controller_key_id: KEY_ID.into(),
        controller_verifying_key_hex: encode_hex(signing_key().verifying_key().as_bytes()),
        catalog_path: "/tmp/cu-worker/catalog.json".into(),
        image_cache_root: "/tmp/cu-worker/images".into(),
        generation_floor_root: "/tmp/cu-worker/floor".into(),
        overlay_root: "/tmp/cu-worker/overlays".into(),
        runtime_root: "/tmp/cu-worker/runtime".into(),
        cgroup_root: "/sys/fs/cgroup/koolade-cu".into(),
        checkpoint_path: "/tmp/cu-worker/checkpoint.json".into(),
        trusted_image_digests: [format!("sha256:{}", "a".repeat(64))].into_iter().collect(),
        max_vcpus: 4,
        max_memory_bytes: 2 * 1024 * 1024 * 1024,
        max_disk_bytes: 4 * 1024 * 1024 * 1024,
        max_parallel_sessions: 1,
    }
}

fn session(
    key: &SigningKey,
    lease_generation: u64,
    cancellation_generation: u64,
) -> ComputerSessionV1 {
    let identity = ComputerIdentityV1 {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        user_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
    };
    let allowed_apps = std::collections::BTreeSet::from(["chromium".to_owned()]);
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: allowed_apps.clone(),
        authorized_operations: std::collections::BTreeSet::from([
            ComputerOperationV1::ObserveScreen,
        ]),
        approval_generation: 1,
        resource_budget: ComputerResourceBudgetV1 {
            cpu_millis: 2_000,
            memory_bytes: 1024 * 1024 * 1024,
            disk_bytes: 2 * 1024 * 1024 * 1024,
            time_limit_ms: 60_000,
            action_limit: 100,
        },
    };
    let mut session = ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: 1,
        identity,
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::RemoteVmWorker {
            worker_id: WORKER_ID.into(),
            machine_id: "00000000-0000-4000-8000-000000000098".into(),
            trust: crate::computer_use::ComputerRemoteWorkerTrustV1 {
                ssh_host_key_fingerprint: fingerprint(),
                worker_attestation_key_id: "00000000-0000-4000-8000-000000000009".into(),
                worker_attestation_verifying_key_hex: encode_hex(
                    &ed25519_dalek::SigningKey::from_bytes(&[9; 32])
                        .verifying_key()
                        .to_bytes(),
                ),
                transport_protocol_version:
                    crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
                guest_protocol_version:
                    crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
            },
        },
        image_digest: format!("sha256:{}", "a".repeat(64)),
        image_manifest_digest: format!("sha256:{}", "b".repeat(64)),
        image_signer_key_id: "00000000-0000-4000-8000-000000000006".into(),
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps,
        policy,
        lease: ComputerLeaseV1 {
            schema_version: 1,
            owner_id: "00000000-0000-4000-8000-000000000003".into(),
            lease_id: "00000000-0000-4000-8000-000000000007".into(),
            generation: lease_generation,
            issued_at_unix_ms: 1,
            expires_at_unix_ms: NOW + 60_000 + (lease_generation * 1_000),
            cancellation_generation,
        },
        lifecycle: ComputerLifecycleV1::Delegated,
        action_sequence: 0,
        approval_generation: 1,
        cancellation_generation,
        evidence_refs: vec![],
        controller_proof: crate::computer_use::ComputerControllerProofV1 {
            signer_key_id: KEY_ID.into(),
            signature_hex: "0".repeat(128),
        },
    };
    session.attest(KEY_ID.into(), key).unwrap();
    session
}

fn request(
    session: &ComputerSessionV1,
    seq: u64,
    id: u128,
    body: Body,
) -> ComputerRemoteWorkerRequestV1 {
    ComputerRemoteWorkerRequestV1 {
        protocol_version: crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        worker_id: WORKER_ID.into(),
        ssh_host_key_fingerprint: fingerprint(),
        session_id: session.identity.session_id.clone(),
        lease_id: session.lease.lease_id.clone(),
        lease_generation: session.lease.generation,
        approval_generation: session.approval_generation,
        cancellation_generation: session.cancellation_generation,
        transport_sequence: seq,
        request_id: uuid::Uuid::from_u128(id).to_string(),
        body,
    }
}

fn start_request(session: &ComputerSessionV1) -> ComputerRemoteWorkerRequestV1 {
    request(
        session,
        1,
        0x501,
        Body::Start {
            session: session.clone(),
        },
    )
}

fn start_fence(key: &SigningKey, session: ComputerSessionV1) -> ComputerRemoteWorkerSessionFenceV1 {
    let registration = enrollment();
    let identity = VerifiedComputerRemoteWorkerIdentityV1::fixture(WORKER_ID, &fingerprint());
    ComputerRemoteWorkerSessionFenceV1::accept_start(
        &start_request(&session),
        &registration,
        &identity,
        KEY_ID,
        &key.verifying_key(),
        NOW,
    )
    .unwrap()
}

fn observe_action(session: &ComputerSessionV1) -> crate::computer_use::ComputerActionV1 {
    crate::computer_use::ComputerActionV1 {
        schema_version: 1,
        session_id: session.identity.session_id.clone(),
        action_sequence: 1,
        idempotency_key: "00000000-0000-4000-8000-000000000901".into(),
        actor: ComputerActionActorV1::Agent,
        lease_generation: session.lease.generation,
        approval_generation: session.approval_generation,
        cancellation_generation: session.cancellation_generation,
        operation: ComputerOperationV1::ObserveScreen,
        payload: crate::computer_use::ComputerActionPayloadV1::ObserveScreen,
    }
}

#[test]
fn signed_session_host_identity_and_protocol_must_all_match_enrollment() {
    let key = signing_key();
    let session = session(&key, 1, 1);
    assert!(
        start_fence(&key, session.clone())
            .checkpoint()
            .next_transport_sequence
            == 2
    );

    let mut bad_registration = enrollment();
    bad_registration.ssh_host_key_fingerprint = format!("SHA256:{}", "B".repeat(43));
    let identity = VerifiedComputerRemoteWorkerIdentityV1::fixture(WORKER_ID, &fingerprint());
    assert!(
        ComputerRemoteWorkerSessionFenceV1::accept_start(
            &start_request(&session),
            &bad_registration,
            &identity,
            KEY_ID,
            &key.verifying_key(),
            NOW,
        )
        .is_err()
    );

    let wrong_key = SigningKey::from_bytes(&[8; 32]);
    assert!(
        ComputerRemoteWorkerSessionFenceV1::accept_start(
            &start_request(&session),
            &enrollment(),
            &identity,
            KEY_ID,
            &wrong_key.verifying_key(),
            NOW,
        )
        .is_err()
    );
}

#[test]
fn lost_action_is_checkpointed_as_unknown_and_never_replayed_after_restart() {
    let key = signing_key();
    let original = session(&key, 1, 1);
    let mut fence = start_fence(&key, original.clone());
    let action = observe_action(&original);
    let action_request = request(&original, 2, 0x502, Body::Action { action });
    fence
        .accept_request(&action_request, KEY_ID, &key.verifying_key(), NOW)
        .unwrap();
    assert_eq!(fence.mark_transport_lost(), Some(1));

    let checkpoint = fence.checkpoint();
    let encoded = serde_json::to_vec(&checkpoint).unwrap();
    let checkpoint: ComputerRemoteWorkerCheckpointV1 = serde_json::from_slice(&encoded).unwrap();
    let identity = VerifiedComputerRemoteWorkerIdentityV1::fixture(WORKER_ID, &fingerprint());
    let mut recovered = ComputerRemoteWorkerSessionFenceV1::restore(
        original.clone(),
        checkpoint,
        &enrollment(),
        &identity,
        KEY_ID,
        &key.verifying_key(),
        NOW,
    )
    .unwrap();
    assert!(
        recovered
            .accept_request(&action_request, KEY_ID, &key.verifying_key(), NOW,)
            .is_err()
    );
    let retry_with_new_id = request(
        &original,
        3,
        0x503,
        Body::Action {
            action: observe_action(&original),
        },
    );
    assert!(
        recovered
            .accept_request(&retry_with_new_id, KEY_ID, &key.verifying_key(), NOW,)
            .unwrap_err()
            .contains("outcome is unknown")
    );
    let replacement = session(&key, 2, 2);
    let reconnect = request(
        &replacement,
        0,
        0x50b,
        Body::Recover {
            replacement_session: replacement.clone(),
            last_event_sequence: 0,
        },
    );
    recovered
        .accept_request(&reconnect, KEY_ID, &key.verifying_key(), NOW)
        .unwrap();
    assert_eq!(recovered.checkpoint().uncertain_action_sequence, Some(1));
    let after_reconnect = request(
        &replacement,
        1,
        0x50c,
        Body::Action {
            action: observe_action(&replacement),
        },
    );
    assert!(
        recovered
            .accept_request(&after_reconnect, KEY_ID, &key.verifying_key(), NOW)
            .unwrap_err()
            .contains("outcome is unknown")
    );
}

#[test]
fn cancellation_and_reconnect_advance_session_lease_fences() {
    let key = signing_key();
    let original = session(&key, 1, 1);
    let mut fence = start_fence(&key, original.clone());
    let cancelled = session(&key, 2, 2);
    let cancel = request(
        &original,
        2,
        0x504,
        Body::Cancel {
            replacement_session: cancelled.clone(),
        },
    );
    fence
        .accept_request(&cancel, KEY_ID, &key.verifying_key(), NOW)
        .unwrap();
    let denied_action = request(
        &cancelled,
        3,
        0x505,
        Body::Action {
            action: observe_action(&cancelled),
        },
    );
    assert!(
        fence
            .accept_request(&denied_action, KEY_ID, &key.verifying_key(), NOW)
            .is_err()
    );

    let recovered = session(&key, 3, 3);
    let reconnect = request(
        &cancelled,
        3,
        0x506,
        Body::Recover {
            replacement_session: recovered.clone(),
            last_event_sequence: 0,
        },
    );
    fence
        .accept_request(&reconnect, KEY_ID, &key.verifying_key(), NOW)
        .unwrap();
    let action = request(
        &recovered,
        4,
        0x507,
        Body::Action {
            action: observe_action(&recovered),
        },
    );
    assert!(
        fence
            .accept_request(&action, KEY_ID, &key.verifying_key(), NOW)
            .is_ok()
    );
}

#[test]
fn a_new_ssh_stream_uses_one_generation_fenced_sequence_zero_recovery() {
    let key = signing_key();
    let original = session(&key, 1, 1);
    assert!(
        request(
            &original,
            0,
            0x50a,
            Body::Action {
                action: observe_action(&original),
            },
        )
        .validate_shape()
        .is_err()
    );
    let mut fence = start_fence(&key, original.clone());
    let renewed = session(&key, 2, 2);
    let recovery = request(
        &renewed,
        0,
        0x508,
        Body::Recover {
            replacement_session: renewed.clone(),
            last_event_sequence: 0,
        },
    );
    fence
        .accept_request(&recovery, KEY_ID, &key.verifying_key(), NOW)
        .unwrap();
    assert_eq!(fence.checkpoint().next_transport_sequence, 1);

    let action = request(
        &renewed,
        1,
        0x509,
        Body::Action {
            action: observe_action(&renewed),
        },
    );
    fence
        .accept_request(&action, KEY_ID, &key.verifying_key(), NOW)
        .unwrap();
    assert!(
        fence
            .accept_request(&recovery, KEY_ID, &key.verifying_key(), NOW)
            .is_err()
    );
}

#[test]
fn concurrent_session_identity_and_transport_sequence_are_not_interchangeable() {
    let key = signing_key();
    let session_a = session(&key, 1, 1);
    let mut fence_a = start_fence(&key, session_a.clone());
    let mut session_b = session(&key, 1, 1);
    session_b.identity.session_id = "00000000-0000-4000-8000-000000000104".into();
    session_b.policy.identity = session_b.identity.clone();
    session_b.attest(KEY_ID.into(), &key).unwrap();
    let mut fence_b = start_fence(&key, session_b.clone());
    let a_action = request(
        &session_a,
        2,
        0x508,
        Body::Action {
            action: observe_action(&session_a),
        },
    );
    fence_a
        .accept_request(&a_action, KEY_ID, &key.verifying_key(), NOW)
        .unwrap();
    assert!(
        fence_b
            .accept_request(&a_action, KEY_ID, &key.verifying_key(), NOW)
            .is_err()
    );
}
