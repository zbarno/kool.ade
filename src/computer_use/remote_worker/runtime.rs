//! Local-KVM implementation of the separately served Computer Use worker.
//!
//! The overlay provider is intentionally injected: a worker must resolve the
//! signed image catalog and its own private cache before it can allocate a VM.

use std::{
    collections::HashMap,
    io::{self, Read},
    sync::Arc,
    time::Duration,
};

use crate::computer_use::{
    ComputerBackend, ComputerBackendErrorV1, ComputerGuestReadinessV1, ComputerMachineHandleV1,
    ComputerSessionV1, guest_image::GuestImageOverlayV1,
};

use super::{
    ComputerRemoteWorkerHostConfigV1, ComputerRemoteWorkerOperationV1 as Operation,
    ComputerRemoteWorkerResultV1 as ResultValue, ComputerRemoteWorkerRuntimeV1,
};

pub trait ComputerRemoteWorkerOverlayProviderV1: Send {
    fn create_overlay(&mut self, session: &ComputerSessionV1) -> io::Result<GuestImageOverlayV1>;
}

/// Production overlay provider built from an explicitly provisioned local
/// cache, signed catalog bytes, durable catalog floor and controller trust.
pub struct SignedCatalogRemoteWorkerOverlayProviderV1 {
    cache: crate::computer_use::guest_image::GuestImageCacheV1,
    manifest_bytes: Vec<u8>,
    trust: crate::computer_use::guest_image::GuestImageManifestTrustV1,
    floor: crate::computer_use::guest_image::GuestImageCatalogGenerationFloorV1,
    overlay_root: std::path::PathBuf,
    controller_key_id: String,
    controller_key: ed25519_dalek::VerifyingKey,
    qemu_tool: crate::computer_use::guest_image::QemuImageToolV1,
}

impl SignedCatalogRemoteWorkerOverlayProviderV1 {
    pub fn open(
        cache_root: std::path::PathBuf,
        manifest_path: &std::path::Path,
        generation_floor_root: std::path::PathBuf,
        overlay_root: std::path::PathBuf,
        controller_key_id: String,
        controller_key: ed25519_dalek::VerifyingKey,
    ) -> io::Result<Self> {
        let metadata = std::fs::symlink_metadata(manifest_path)?;
        if !metadata.is_file()
            || metadata.len() > crate::computer_use::guest_image::MAX_MANIFEST_BYTES as u64
        {
            return Err(invalid_data(
                "signed guest image catalog file is unsafe or too large",
            ));
        }
        let mut manifest_bytes = Vec::with_capacity(metadata.len() as usize);
        std::fs::File::open(manifest_path)?
            .take(crate::computer_use::guest_image::MAX_MANIFEST_BYTES as u64 + 1)
            .read_to_end(&mut manifest_bytes)?;
        if manifest_bytes.len() > crate::computer_use::guest_image::MAX_MANIFEST_BYTES {
            return Err(invalid_data(
                "signed guest image catalog file is unsafe or too large",
            ));
        }
        let cache = crate::computer_use::guest_image::GuestImageCacheV1::open_existing(cache_root)
            .map_err(|_| invalid_data("provisioned guest image cache is unavailable"))?;
        let floor = crate::computer_use::guest_image::GuestImageCatalogGenerationFloorV1::open(
            generation_floor_root,
        )
        .map_err(|_| invalid_data("durable catalog generation floor is unavailable"))?;
        let trust = crate::computer_use::guest_image::application_guest_image_manifest_trust_v1()
            .map_err(|_| invalid_data("application guest image trust is unavailable"))?;
        let qemu_tool = crate::computer_use::guest_image::probe_host_qemu_image_tool()
            .map_err(|_| invalid_data("qemu-img is unavailable on the remote worker"))?;
        Ok(Self {
            cache,
            manifest_bytes,
            trust,
            floor,
            overlay_root,
            controller_key_id,
            controller_key,
            qemu_tool,
        })
    }
}

impl ComputerRemoteWorkerOverlayProviderV1 for SignedCatalogRemoteWorkerOverlayProviderV1 {
    fn create_overlay(&mut self, session: &ComputerSessionV1) -> io::Result<GuestImageOverlayV1> {
        crate::computer_use::guest_image::create_guest_image_overlay(
            &self.cache,
            crate::computer_use::guest_image::GuestImageOverlayRequestV1 {
                manifest_bytes: &self.manifest_bytes,
                trust: &self.trust,
                session,
                expected_architecture:
                    crate::computer_use::guest_image::ComputerGuestArchitectureV1::X86_64,
                controller_key_id: &self.controller_key_id,
                controller_key: &self.controller_key,
                generation_floor: &self.floor,
                overlay_root: &self.overlay_root,
                qemu_tool: &self.qemu_tool,
            },
        )
        .map_err(|_| invalid_data("signed catalog rejected remote session overlay"))
    }
}

struct SessionRuntimeState {
    handle: ComputerMachineHandleV1,
    authenticated: Option<crate::computer_use::guest_protocol::AuthenticatedGuestChannelV1>,
    journal: Option<crate::computer_use::guest_protocol::GuestActionJournalV1>,
}

/// Worker RPC dispatcher backed by the existing, target-bound local KVM
/// implementation. It never receives controller signing keys or host paths.
pub struct LocalKvmComputerRemoteWorkerRuntimeV1<P> {
    config: ComputerRemoteWorkerHostConfigV1,
    backend: Arc<crate::computer_use::LocalKvmComputerBackend>,
    overlays: P,
    sessions: HashMap<String, SessionRuntimeState>,
}

impl<P: ComputerRemoteWorkerOverlayProviderV1> LocalKvmComputerRemoteWorkerRuntimeV1<P> {
    pub fn new(
        config: ComputerRemoteWorkerHostConfigV1,
        backend: Arc<crate::computer_use::LocalKvmComputerBackend>,
        overlays: P,
    ) -> io::Result<Self> {
        config.validate().map_err(invalid_data)?;
        Ok(Self {
            config,
            backend,
            overlays,
            sessions: HashMap::new(),
        })
    }

    fn session_handle(&self, session: &ComputerSessionV1) -> io::Result<ComputerMachineHandleV1> {
        let state = self
            .sessions
            .get(&session.identity.session_id)
            .ok_or_else(|| invalid_data("worker session is not allocated"))?;
        if state.handle.lease_id() != session.lease.lease_id
            || state.handle.lease_generation() > session.lease.generation
            || state.handle.machine_id() != self.config.machine_id
        {
            return Err(invalid_data("worker session handle is stale or mismatched"));
        }
        Ok(state.handle.clone())
    }
}

impl<P: ComputerRemoteWorkerOverlayProviderV1> ComputerRemoteWorkerRuntimeV1
    for LocalKvmComputerRemoteWorkerRuntimeV1<P>
{
    fn advertise(&mut self) -> io::Result<super::ComputerRemoteWorkerAdvertisementV1> {
        let probe = self.backend.probe().map_err(backend_error)?;
        let mut advertisement = super::ComputerRemoteWorkerAdvertisementV1 {
            schema_version: 1,
            worker_id: self.config.worker_id.clone(),
            attestation_key_id: self.config.worker_attestation_key_id.clone(),
            signature_hex: String::new(),
            ssh_host_key_fingerprint: self.config.ssh_host_key_fingerprint.clone(),
            transport_protocol_version:
                crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
            guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
            observed_at_unix_ms: now_unix_ms()?,
            cpu_virtualization_available: probe.kvm_accessible,
            image_digests: self.config.trusted_image_digests.iter().cloned().collect(),
            display_streaming_available: true,
            cpu_millis: u32::from(probe.max_vcpus).saturating_mul(1_000),
            memory_bytes: probe.max_memory_bytes,
            disk_bytes: self.config.max_disk_bytes,
            encrypted_personal_storage_available: false,
            parallel_slots: probe
                .parallel_slots_per_machine
                .min(self.config.max_parallel_sessions),
        };
        self.config
            .sign_advertisement(&mut advertisement)
            .map_err(invalid_data)?;
        Ok(advertisement)
    }

    fn start(
        &mut self,
        session: &ComputerSessionV1,
    ) -> io::Result<(
        ComputerMachineHandleV1,
        crate::computer_use::ComputerBackendEventV1,
    )> {
        let crate::computer_use::ComputerTargetV1::RemoteVmWorker {
            worker_id,
            machine_id,
            ..
        } = &session.target
        else {
            return Err(invalid_data(
                "worker session does not target a remote VM worker",
            ));
        };
        if worker_id != &self.config.worker_id
            || machine_id != &self.config.machine_id
            || !self
                .config
                .trusted_image_digests
                .contains(&session.image_digest)
            || session.policy.resource_budget.cpu_millis > self.config.max_vcpus as u32 * 1_000
            || session.policy.resource_budget.memory_bytes > self.config.max_memory_bytes
            || session.policy.resource_budget.disk_bytes > self.config.max_disk_bytes
        {
            return Err(invalid_data("session exceeds worker enrollment policy"));
        }
        let overlay = self.overlays.create_overlay(session)?;
        let (handle, event) = self
            .backend
            .prepare(session, overlay, now_unix_ms()?)
            .map_err(backend_error)?;
        self.sessions.insert(
            session.identity.session_id.clone(),
            SessionRuntimeState {
                handle: handle.clone(),
                authenticated: None,
                journal: None,
            },
        );
        Ok((handle, event))
    }

    fn operation(
        &mut self,
        session: &ComputerSessionV1,
        operation: &Operation,
    ) -> io::Result<ResultValue> {
        use crate::computer_use::{ComputerBackend as _, ComputerDisplayFrameV1};
        let handle = self.session_handle(session)?;
        let now = now_unix_ms()?;
        let result = match operation {
            Operation::Boot { .. } => ResultValue::BackendEvent(
                self.backend
                    .boot(session, &handle, now)
                    .map_err(backend_error)?,
            ),
            Operation::WaitReady { timeout_ms, .. } => ResultValue::BackendEvent(
                self.backend
                    .wait_ready(
                        session,
                        &handle,
                        Duration::from_millis(*timeout_ms as u64),
                        now,
                    )
                    .map_err(backend_error)?,
            ),
            Operation::Suspend { .. } => ResultValue::BackendEvent(
                self.backend
                    .suspend(session, &handle, now)
                    .map_err(backend_error)?,
            ),
            Operation::Resume { .. } => ResultValue::BackendEvent(
                self.backend
                    .resume(session, &handle, now)
                    .map_err(backend_error)?,
            ),
            Operation::Stop { .. } => ResultValue::BackendEvent(
                self.backend
                    .stop(session, &handle, now)
                    .map_err(backend_error)?,
            ),
            Operation::Collect { .. } => ResultValue::MachineRecord(
                self.backend
                    .collect(session, &handle, now)
                    .map_err(backend_error)?,
            ),
            Operation::Snapshot { .. } => ResultValue::MachineRecord(
                self.backend
                    .snapshot(session, &handle, now)
                    .map_err(backend_error)?,
            ),
            Operation::Release { .. } => {
                let event = self
                    .backend
                    .release(session, &handle, now)
                    .map_err(backend_error)?;
                self.sessions.remove(&session.identity.session_id);
                ResultValue::BackendEvent(event)
            }
            Operation::CaptureDisplay { .. } => ResultValue::DisplayFrame(
                self.backend
                    .capture_display_frame(session, &handle, now)
                    .map_err(backend_error)?,
            ),
            Operation::SendDisplayInput { input, .. } => {
                self.backend
                    .send_display_input(session, &handle, input, now)
                    .map_err(backend_error)?;
                ResultValue::Acknowledged
            }
            Operation::AuthenticateGuest { .. } => {
                let channel = self
                    .backend
                    .authenticate_guest(session, &handle, now)
                    .map_err(backend_error)?;
                let readiness: ComputerGuestReadinessV1 = channel.readiness();
                let capabilities = channel.capabilities().clone();
                let transcript_sha256 = channel.transcript_sha256().to_owned();
                let journal = self
                    .backend
                    .open_guest_action_journal(session, &handle, now)
                    .map_err(backend_error)?;
                let state = self
                    .sessions
                    .get_mut(&session.identity.session_id)
                    .expect("session was checked");
                state.authenticated = Some(channel);
                state.journal = Some(journal);
                ResultValue::GuestAuthenticated {
                    readiness,
                    capabilities,
                    transcript_sha256,
                }
            }
            Operation::MarkGuestReady { .. } => ResultValue::Acknowledged,
            Operation::GuestAction {
                action,
                deadline_unix_ms,
                ..
            } => {
                let state = self
                    .sessions
                    .get_mut(&session.identity.session_id)
                    .expect("session was checked");
                let channel = state
                    .authenticated
                    .as_mut()
                    .ok_or_else(|| invalid_data("guest has not authenticated"))?;
                let journal = state
                    .journal
                    .as_ref()
                    .ok_or_else(|| invalid_data("guest action journal is unavailable"))?;
                let outcome = channel
                    .execute_action(
                        session,
                        action.actor,
                        action,
                        *deadline_unix_ms,
                        now,
                        journal,
                    )
                    .map_err(|_| {
                        io::Error::other("authenticated guest action failed or has unknown outcome")
                    })?;
                ResultValue::GuestAction(outcome)
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
                let state = self
                    .sessions
                    .get_mut(&session.identity.session_id)
                    .expect("session was checked");
                let channel = state
                    .authenticated
                    .as_mut()
                    .ok_or_else(|| invalid_data("guest has not authenticated"))?;
                let (next_offset, complete) = channel
                    .write_artifact_chunk(
                        session,
                        transfer_id,
                        file_name,
                        *offset,
                        *total_bytes,
                        sha256,
                        bytes,
                        now,
                    )
                    .map_err(|_| invalid_data("guest artifact write was refused"))?;
                ResultValue::ArtifactWriteAck {
                    transfer_id: transfer_id.clone(),
                    next_offset,
                    complete,
                }
            }
            Operation::ReadArtifactChunk {
                transfer_id,
                file_name,
                offset,
                ..
            } => {
                let state = self
                    .sessions
                    .get_mut(&session.identity.session_id)
                    .expect("session was checked");
                let channel = state
                    .authenticated
                    .as_mut()
                    .ok_or_else(|| invalid_data("guest has not authenticated"))?;
                let (total_bytes, sha256, bytes, complete) = channel
                    .read_artifact_chunk(session, transfer_id, file_name, *offset, now)
                    .map_err(|_| invalid_data("guest artifact read was refused"))?;
                ResultValue::ArtifactReadChunk {
                    transfer_id: transfer_id.clone(),
                    offset: *offset,
                    total_bytes,
                    sha256,
                    bytes,
                    complete,
                }
            }
            Operation::AbortArtifactTransfer { transfer_id, .. } => {
                let state = self
                    .sessions
                    .get_mut(&session.identity.session_id)
                    .expect("session was checked");
                let channel = state
                    .authenticated
                    .as_mut()
                    .ok_or_else(|| invalid_data("guest has not authenticated"))?;
                channel
                    .abort_artifact_transfer(session, transfer_id, now)
                    .map_err(|_| invalid_data("guest artifact abort was refused"))?;
                ResultValue::Acknowledged
            }
        };
        let _ = std::marker::PhantomData::<ComputerDisplayFrameV1>;
        Ok(result)
    }

    fn cancel(
        &mut self,
        session: &ComputerSessionV1,
    ) -> io::Result<crate::computer_use::ComputerBackendEventV1> {
        let handle = self.session_handle(session)?;
        let event = self
            .backend
            .cancel(session, &handle, now_unix_ms()?)
            .map_err(backend_error)?;
        Ok(event)
    }

    fn recover(&mut self, session: &ComputerSessionV1) -> io::Result<()> {
        let events = self.backend.recover().map_err(backend_error)?;
        let event = events
            .into_iter()
            .find(|event| event.session_id == session.identity.session_id)
            .ok_or_else(|| invalid_data("worker recovery did not find the fenced session"))?;
        if event.lease_generation > session.lease.generation {
            return Err(invalid_data("recovered worker session has a newer lease"));
        }
        let handle = ComputerMachineHandleV1::from_remote(
            session.identity.session_id.clone(),
            session.lease.lease_id.clone(),
            session.lease.generation,
            self.config.machine_id.clone(),
        );
        self.sessions.insert(
            session.identity.session_id.clone(),
            SessionRuntimeState {
                handle,
                authenticated: None,
                journal: None,
            },
        );
        Ok(())
    }

    fn release(
        &mut self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
    ) -> io::Result<()> {
        self.backend
            .release(session, handle, now_unix_ms()?)
            .map_err(backend_error)?;
        self.sessions.remove(&session.identity.session_id);
        Ok(())
    }
}

fn backend_error(error: ComputerBackendErrorV1) -> io::Error {
    io::Error::other(format!("Computer Use worker backend failed: {error}"))
}
fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn now_unix_ms() -> io::Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .map_err(|_| io::Error::other("system clock is before Unix epoch"))
}
