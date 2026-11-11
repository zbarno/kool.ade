use super::*;
use crate::computer_use::remote_worker::transport::VerifiedComputerRemoteWorkerIdentityV1;
use crate::computer_use::{
    ComputerActionPayloadV1, ComputerBackendEventKindV1, ComputerCapabilityV1, ComputerIdentityV1,
    ComputerLeaseV1, ComputerLifecycleV1, ComputerNetworkProfileV1, ComputerOperationV1,
    ComputerPolicyV1, ComputerResourceBudgetV1, ComputerSessionClassV1, ComputerTargetV1,
    artifacts::{ComputerArtifactFlowV1, ComputerTransferGrantRequestV1},
    remote_worker::{
        ComputerRemoteWorkerCheckpointStoreV1, ComputerRemoteWorkerCheckpointV1,
        ComputerRemoteWorkerHostConfigV1, ComputerRemoteWorkerOperationV1 as Operation,
        ComputerRemoteWorkerRequestBodyV1 as Body, ComputerRemoteWorkerRequestV1 as Request,
        ComputerRemoteWorkerResultV1 as ResultValue, ComputerRemoteWorkerRuntimeV1,
        serve_computer_remote_worker_stdio_v1,
    },
};
use ed25519_dalek::SigningKey;
use sha2::Digest;
use std::{
    collections::{BTreeMap, HashMap},
    io,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};

const WORKER_ID: &str = "cu-worker-test";
const MACHINE_ID: &str = "00000000-0000-4000-8000-000000000098";
const CONTROLLER_KEY_ID: &str = "00000000-0000-4000-8000-000000000008";
const WORKER_KEY_ID: &str = "00000000-0000-4000-8000-000000000009";
const IMAGE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn fingerprint() -> String {
    format!("SHA256:{}", "A".repeat(43))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn host_config(key: &SigningKey) -> ComputerRemoteWorkerHostConfigV1 {
    ComputerRemoteWorkerHostConfigV1 {
        schema_version: 1,
        enabled: true,
        worker_id: WORKER_ID.into(),
        machine_id: MACHINE_ID.into(),
        ssh_host_key_fingerprint: fingerprint(),
        worker_attestation_key_id: WORKER_KEY_ID.into(),
        worker_attestation_private_key_hex: hex(&[9; 32]),
        controller_key_id: CONTROLLER_KEY_ID.into(),
        controller_verifying_key_hex: hex(key.verifying_key().as_bytes()),
        catalog_path: "/tmp/cu-worker/catalog.json".into(),
        image_cache_root: "/tmp/cu-worker/images".into(),
        generation_floor_root: "/tmp/cu-worker/floor".into(),
        overlay_root: "/tmp/cu-worker/overlays".into(),
        runtime_root: "/tmp/cu-worker/runtime".into(),
        cgroup_root: "/sys/fs/cgroup/koolade-cu".into(),
        checkpoint_path: "/tmp/cu-worker/checkpoint.json".into(),
        trusted_image_digests: [IMAGE.into()].into_iter().collect(),
        max_vcpus: 4,
        max_memory_bytes: 2 * 1024 * 1024 * 1024,
        max_disk_bytes: 4 * 1024 * 1024 * 1024,
        max_parallel_sessions: 1,
    }
}

fn controller_enrollment() -> ComputerRemoteWorkerEnrollmentV1 {
    let worker_key = SigningKey::from_bytes(&[9; 32]);
    ComputerRemoteWorkerEnrollmentV1 {
        schema_version: 1,
        enabled: true,
        worker_id: WORKER_ID.into(),
        machine_id: MACHINE_ID.into(),
        ssh_host_alias: "worker-test.invalid".into(),
        ssh_principal: "computer-worker".into(),
        known_hosts_path: "/etc/ssh/ssh_known_hosts".into(),
        ssh_binary_path: "/usr/bin/ssh".into(),
        worker_config_path: "/etc/koolade/computer-use-worker.json".into(),
        ssh_host_key_fingerprint: fingerprint(),
        worker_attestation_key_id: WORKER_KEY_ID.into(),
        worker_attestation_verifying_key_hex: hex(worker_key.verifying_key().as_bytes()),
        transport_protocol_version:
            crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        trusted_image_digests: [IMAGE.into()].into_iter().collect(),
        max_vcpus: 4,
        max_memory_bytes: 2 * 1024 * 1024 * 1024,
        max_disk_bytes: 4 * 1024 * 1024 * 1024,
        max_parallel_sessions: 1,
        enrolled_at_unix_ms: 1,
    }
}

fn signed_session(key: &SigningKey, now: u64) -> ComputerSessionV1 {
    let identity = ComputerIdentityV1 {
        project_id: uuid::Uuid::new_v4().to_string(),
        task_id: uuid::Uuid::new_v4().to_string(),
        user_id: uuid::Uuid::new_v4().to_string(),
        session_id: uuid::Uuid::new_v4().to_string(),
    };
    let allowed_apps = std::collections::BTreeSet::from(["chromium".to_owned()]);
    let budget = ComputerResourceBudgetV1 {
        cpu_millis: 2_000,
        memory_bytes: 1024 * 1024 * 1024,
        disk_bytes: 2 * 1024 * 1024 * 1024,
        time_limit_ms: 60_000,
        action_limit: 100,
    };
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: allowed_apps.clone(),
        authorized_operations: [ComputerOperationV1::ObserveScreen].into_iter().collect(),
        approval_generation: 1,
        resource_budget: budget,
    };
    let mut session = ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: now,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::RemoteVmWorker {
            worker_id: WORKER_ID.into(),
            machine_id: MACHINE_ID.into(),
            trust: crate::computer_use::ComputerRemoteWorkerTrustV1 {
                ssh_host_key_fingerprint: fingerprint(),
                worker_attestation_key_id: WORKER_KEY_ID.into(),
                worker_attestation_verifying_key_hex: hex(&SigningKey::from_bytes(&[9; 32])
                    .verifying_key()
                    .to_bytes()),
                transport_protocol_version:
                    crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
                guest_protocol_version:
                    crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
            },
        },
        image_digest: IMAGE.into(),
        image_manifest_digest:
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        image_signer_key_id: "00000000-0000-4000-8000-000000000006".into(),
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps,
        policy,
        lease: ComputerLeaseV1 {
            schema_version: 1,
            owner_id: identity.user_id.clone(),
            lease_id: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + 60_000,
            cancellation_generation: 1,
        },
        lifecycle: ComputerLifecycleV1::Delegated,
        action_sequence: 0,
        approval_generation: 1,
        cancellation_generation: 1,
        evidence_refs: vec![],
        controller_proof: crate::computer_use::ComputerControllerProofV1 {
            signer_key_id: CONTROLLER_KEY_ID.into(),
            signature_hex: "0".repeat(128),
        },
    };
    session.attest(CONTROLLER_KEY_ID.into(), key).unwrap();
    session
}

fn request(session: &ComputerSessionV1, sequence: u64, body: Body) -> Request {
    Request {
        protocol_version: crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
        worker_id: WORKER_ID.into(),
        ssh_host_key_fingerprint: fingerprint(),
        session_id: session.identity.session_id.clone(),
        lease_id: session.lease.lease_id.clone(),
        lease_generation: session.lease.generation,
        approval_generation: session.approval_generation,
        cancellation_generation: session.cancellation_generation,
        transport_sequence: sequence,
        request_id: uuid::Uuid::new_v4().to_string(),
        body,
    }
}

#[derive(Clone, Default)]
struct SharedCheckpoint(Arc<Mutex<Option<(ComputerSessionV1, ComputerRemoteWorkerCheckpointV1)>>>);

impl ComputerRemoteWorkerCheckpointStoreV1 for SharedCheckpoint {
    fn load(
        &mut self,
    ) -> io::Result<Option<(ComputerSessionV1, ComputerRemoteWorkerCheckpointV1)>> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn save(
        &mut self,
        session: &ComputerSessionV1,
        checkpoint: &ComputerRemoteWorkerCheckpointV1,
    ) -> io::Result<()> {
        *self.0.lock().unwrap() = Some((session.clone(), checkpoint.clone()));
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[derive(Default)]
struct FakeGuest {
    files: BTreeMap<String, Vec<u8>>,
    writes: HashMap<String, (String, u64, String, Vec<u8>)>,
    operations: Vec<&'static str>,
}

struct FakeRuntime {
    config: ComputerRemoteWorkerHostConfigV1,
    guest: Arc<Mutex<FakeGuest>>,
}

impl ComputerRemoteWorkerRuntimeV1 for FakeRuntime {
    fn advertise(&mut self) -> io::Result<ComputerRemoteWorkerAdvertisementV1> {
        let mut ad = ComputerRemoteWorkerAdvertisementV1 {
            schema_version: 1,
            worker_id: self.config.worker_id.clone(),
            attestation_key_id: self.config.worker_attestation_key_id.clone(),
            signature_hex: String::new(),
            ssh_host_key_fingerprint: self.config.ssh_host_key_fingerprint.clone(),
            transport_protocol_version:
                crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
            guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
            observed_at_unix_ms: 1,
            cpu_virtualization_available: true,
            image_digests: vec![IMAGE.into()],
            display_streaming_available: true,
            cpu_millis: 4_000,
            memory_bytes: self.config.max_memory_bytes,
            disk_bytes: self.config.max_disk_bytes,
            encrypted_personal_storage_available: false,
            parallel_slots: 1,
        };
        self.config
            .sign_advertisement(&mut ad)
            .map_err(invalid_data)?;
        Ok(ad)
    }

    fn start(
        &mut self,
        session: &ComputerSessionV1,
    ) -> io::Result<(
        ComputerMachineHandleV1,
        crate::computer_use::ComputerBackendEventV1,
    )> {
        self.guest.lock().unwrap().operations.push("start");
        let handle = ComputerMachineHandleV1::from_remote(
            session.identity.session_id.clone(),
            session.lease.lease_id.clone(),
            session.lease.generation,
            MACHINE_ID.into(),
        );
        Ok((
            handle,
            event(session, 1, ComputerBackendEventKindV1::Prepared),
        ))
    }

    fn operation(
        &mut self,
        session: &ComputerSessionV1,
        operation: &Operation,
    ) -> io::Result<ResultValue> {
        let mut guest = self.guest.lock().unwrap();
        match operation {
            Operation::Boot { .. } => {
                guest.operations.push("boot");
                Ok(ResultValue::BackendEvent(event(
                    session,
                    2,
                    ComputerBackendEventKindV1::Booted,
                )))
            }
            Operation::CaptureDisplay { .. } => {
                guest.operations.push("display");
                Ok(ResultValue::DisplayFrame(ComputerDisplayFrameV1 {
                    width: 1,
                    height: 1,
                    png: b"synthetic-frame".to_vec(),
                }))
            }
            Operation::GuestAction { action, .. } => {
                guest.operations.push("action");
                Ok(ResultValue::GuestAction(GuestActionOutcomeV1 {
                    reply: crate::computer_use::guest_protocol::GuestActionReplyV1 {
                        action_sequence: action.action_sequence,
                        result:
                            crate::computer_use::guest_protocol::GuestOperationResultV1::Completed,
                    },
                    observation: None,
                }))
            }
            Operation::WriteArtifactChunk {
                transfer_id,
                file_name,
                offset,
                total_bytes,
                sha256,
                bytes,
                ..
            } => {
                guest.operations.push("artifact_write");
                let state = guest.writes.entry(transfer_id.clone()).or_insert_with(|| {
                    (file_name.clone(), *total_bytes, sha256.clone(), Vec::new())
                });
                if state.0 != *file_name
                    || state.1 != *total_bytes
                    || state.2 != *sha256
                    || state.3.len() as u64 != *offset
                {
                    return Err(invalid_data("fake guest write offset mismatch"));
                }
                state.3.extend_from_slice(bytes);
                let next_offset = state.3.len() as u64;
                let complete = next_offset == *total_bytes;
                if complete {
                    if format!("sha256:{:x}", sha2::Sha256::digest(&state.3)) != *sha256 {
                        return Err(invalid_data("fake guest artifact digest mismatch"));
                    }
                    let (name, _, _, content) = guest.writes.remove(transfer_id).unwrap();
                    guest.files.insert(name, content);
                }
                Ok(ResultValue::ArtifactWriteAck {
                    transfer_id: transfer_id.clone(),
                    next_offset,
                    complete,
                })
            }
            Operation::ReadArtifactChunk {
                transfer_id,
                file_name,
                offset,
                ..
            } => {
                guest.operations.push("artifact_read");
                let content = guest
                    .files
                    .get(file_name)
                    .ok_or_else(|| invalid_data("fake guest artifact missing"))?;
                let end = (*offset + 256 * 1024).min(content.len() as u64);
                Ok(ResultValue::ArtifactReadChunk {
                    transfer_id: transfer_id.clone(),
                    offset: *offset,
                    total_bytes: content.len() as u64,
                    sha256: format!("sha256:{:x}", sha2::Sha256::digest(content)),
                    bytes: content[*offset as usize..end as usize].to_vec(),
                    complete: end == content.len() as u64,
                })
            }
            Operation::AbortArtifactTransfer { transfer_id, .. } => {
                guest.operations.push("artifact_abort");
                guest.writes.remove(transfer_id);
                Ok(ResultValue::Acknowledged)
            }
            _ => {
                guest.operations.push("other");
                Ok(ResultValue::Acknowledged)
            }
        }
    }

    fn cancel(
        &mut self,
        session: &ComputerSessionV1,
    ) -> io::Result<crate::computer_use::ComputerBackendEventV1> {
        Ok(event(session, 3, ComputerBackendEventKindV1::Cancelled))
    }

    fn recover(&mut self, _session: &ComputerSessionV1) -> io::Result<()> {
        self.guest.lock().unwrap().operations.push("recover");
        Ok(())
    }

    fn release(
        &mut self,
        _session: &ComputerSessionV1,
        _handle: &ComputerMachineHandleV1,
    ) -> io::Result<()> {
        Ok(())
    }
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

mod support;
use support::event;
mod artifact_bridge;
mod recovery;
