use super::{
    GuestBootConfigV1, GuestCapabilitySetV1, GuestExitReasonV1, GuestHandshakeErrorV1,
    GuestMessageV1, GuestProtocolErrorV1,
    auth::{AuthenticatedGuestEnvelopeV1, guest_handshake, host_handshake},
    frame::read_frame,
};
use crate::computer_use::{
    ComputerCapabilityV1, ComputerControllerProofV1, ComputerIdentityV1, ComputerLeaseV1,
    ComputerLifecycleV1, ComputerNetworkProfileV1, ComputerOperationV1, ComputerPolicyV1,
    ComputerResourceBudgetV1, ComputerSessionClassV1, ComputerSessionV1, ComputerTargetV1,
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    thread,
};

const NOW: u64 = 10_000;

/// Production boundary: host and guest handshakes exchange length-framed
/// authenticated messages on a Unix stream. This proves envelope binding and
/// HMAC/HKDF negotiation, which type-validator tests alone cannot prove. It
/// needs no QEMU, KVM, Docker, SSH, or network setup; setup cost is low. The
/// real virtio-serial and VM identity boundary remains in #392.
#[test]
fn mutual_handshake_binds_session_worker_lease_and_fresh_nonces() {
    let session = session();
    let config = boot_config(&session);
    let guest_caps = capabilities();
    let (host_stream, guest_stream) = UnixStream::pair().unwrap();
    let host_config = config.clone();
    let host_session = session.clone();
    let host = thread::spawn(move || {
        let mut stream = host_stream;
        host_handshake(&mut stream, &host_config, &host_session, NOW).unwrap()
    });
    let mut guest_stream = guest_stream;
    let guest = guest_handshake(&mut guest_stream, &config, guest_caps).unwrap();
    let host = host.join().unwrap();
    assert_eq!(host.transcript_sha256, guest.transcript_sha256);
    assert_eq!(host.capabilities, guest.capabilities);
    assert_eq!(host.boot_id, config.boot_id);
    assert_eq!(host.channel_key.expose(), guest.channel_key.expose());

    let previous_transcript = host.transcript_sha256;
    let (host_stream, mut guest_stream) = UnixStream::pair().unwrap();
    let host_config = config.clone();
    let host_session = session.clone();
    let host = thread::spawn(move || {
        let mut stream = host_stream;
        host_handshake(&mut stream, &host_config, &host_session, NOW).unwrap()
    });
    let guest = guest_handshake(&mut guest_stream, &config, capabilities()).unwrap();
    assert_ne!(host.join().unwrap().transcript_sha256, previous_transcript);
    assert_eq!(guest.boot_id, config.boot_id);
}

#[test]
fn guest_identity_or_lease_mismatch_is_rejected_before_readiness() {
    let session = session();
    let config = boot_config(&session);
    let mut forged = config.clone();
    forged.worker_id = "00000000-0000-4000-8000-000000000099".into();
    let (host_stream, guest_stream) = UnixStream::pair().unwrap();
    let host_config = config.clone();
    let host_session = session.clone();
    let host = thread::spawn(move || {
        let mut stream = host_stream;
        host_handshake(&mut stream, &host_config, &host_session, NOW)
    });
    let mut guest_stream = guest_stream;
    assert_eq!(
        guest_handshake(&mut guest_stream, &forged, capabilities()).err(),
        Some(GuestProtocolErrorV1::Io)
    );
    assert_eq!(
        host.join().unwrap().err(),
        Some(GuestProtocolErrorV1::WrongSession)
    );
}

#[test]
fn copied_hello_and_old_proof_cannot_replay_a_new_host_challenge() {
    let session = session();
    let config = boot_config(&session);
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (host_stream, guest_stream) = UnixStream::pair().unwrap();
    let host_config = config.clone();
    let host_session = session.clone();
    let host = thread::spawn(move || {
        let mut stream = host_stream;
        host_handshake(&mut stream, &host_config, &host_session, NOW).unwrap()
    });
    let mut guest_stream = CaptureStream {
        inner: guest_stream,
        written: captured.clone(),
    };
    guest_handshake(&mut guest_stream, &config, capabilities()).unwrap();
    host.join().unwrap();
    let captured_frames = split_frames(&captured.lock().unwrap());
    assert_eq!(captured_frames.len(), 2);

    let (host_stream, mut replay_stream) = UnixStream::pair().unwrap();
    let replay_config = config.clone();
    let replay_session = session.clone();
    let host = thread::spawn(move || {
        let mut stream = host_stream;
        host_handshake(&mut stream, &replay_config, &replay_session, NOW)
            .err()
            .unwrap()
    });
    replay_stream.write_all(&captured_frames[0]).unwrap();
    let _: AuthenticatedGuestEnvelopeV1 = read_frame(&mut replay_stream).unwrap();
    replay_stream.write_all(&captured_frames[1]).unwrap();
    assert_eq!(
        host.join().unwrap(),
        GuestProtocolErrorV1::AuthenticationFailed
    );
}

#[test]
fn boot_config_debug_never_exposes_the_per_boot_secret() {
    let config = boot_config(&session());
    let debug = format!("{config:?}");
    assert!(!debug.contains(&config.secret_hex));
    assert!(debug.contains("[redacted]"));
    assert_eq!(config.validate(), Ok(()));
    let mut malformed = config;
    malformed.secret_hex.push('g');
    assert_eq!(
        malformed.validate(),
        Err(GuestHandshakeErrorV1::InvalidBinding)
    );
}

#[test]
fn each_vm_boot_receives_a_fresh_boot_id_and_secret() {
    let session = session();
    let first = boot_config(&session);
    let second = boot_config(&session);
    assert_ne!(first.boot_id, second.boot_id);
    assert_ne!(first.secret_hex, second.secret_hex);
}

#[test]
fn guest_exit_reasons_are_versioned_typed_protocol_values() {
    let message = GuestMessageV1::GuestExit {
        reason: GuestExitReasonV1::GuestInteractionFailed,
    };
    let json = serde_json::to_vec(&message).unwrap();
    assert_eq!(
        serde_json::from_slice::<GuestMessageV1>(&json).unwrap(),
        message
    );
}

struct CaptureStream {
    inner: UnixStream,
    written: Arc<Mutex<Vec<u8>>>,
}

impl Read for CaptureStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl Write for CaptureStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.written
            .lock()
            .unwrap()
            .extend_from_slice(&buffer[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn split_frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut offset = 0;
    let mut frames = Vec::new();
    while offset + 4 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let end = offset + 4 + size;
        if end > bytes.len() {
            break;
        }
        frames.push(bytes[offset..end].to_vec());
        offset = end;
    }
    frames
}

fn capabilities() -> GuestCapabilitySetV1 {
    GuestCapabilitySetV1 {
        screen_capture: true,
        pointer_input: true,
        keyboard_input: true,
        accessibility_tree: false,
        app_registry: vec!["chromium".into()],
    }
}

fn boot_config(session: &ComputerSessionV1) -> GuestBootConfigV1 {
    GuestBootConfigV1::issue(
        session.identity.session_id.clone(),
        "00000000-0000-4000-8000-000000000005".into(),
        session.lease.generation,
        session.action_sequence,
        session.lease.expires_at_unix_ms,
        session.allowed_apps.iter().cloned().collect(),
    )
    .unwrap()
}

fn session() -> ComputerSessionV1 {
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
    ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: 1,
        identity,
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::LocalKvm {
            machine_id: "00000000-0000-4000-8000-000000000005".into(),
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
            generation: 1,
            issued_at_unix_ms: 1,
            expires_at_unix_ms: NOW + 60_000,
            cancellation_generation: 1,
        },
        lifecycle: ComputerLifecycleV1::Booting,
        action_sequence: 0,
        approval_generation: 1,
        cancellation_generation: 1,
        evidence_refs: vec![],
        controller_proof: ComputerControllerProofV1 {
            signer_key_id: "00000000-0000-4000-8000-000000000008".into(),
            signature_hex: "0".repeat(128),
        },
    }
}
