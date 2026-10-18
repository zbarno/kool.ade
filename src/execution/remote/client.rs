//! Controller side client for the authenticated worker protocol.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{
    execution::{
        backend::BackendProbeV1,
        errors::{ExecutionError, ExecutionErrorKind},
    },
    persistence::remote_requests,
};

use super::{
    protocol::{
        EnvelopeKindV1 as Kind, EnvelopeV1, PROTOCOL_VERSION_V1, SYSTEM_IMAGE_SESSION_ID_V1,
    },
    ssh::SshWorkerConfig,
    transport::{RemoteRpcTransport, SshTransport},
};
mod exchange;
mod health;
mod helpers;
mod reconnect;
pub(super) use helpers::matches_response;
use helpers::{
    disconnected, incompatible, negotiated_probe, unavailable, unavailable_probe, verified,
};

const CONTROL_ID: &str = "00000000-0000-4000-8000-000000000000";
const HEALTH_REFRESH_INTERVAL: Duration = Duration::from_secs(15);

fn new_capability_key() -> [u8; 32] {
    let mut key = [0_u8; 32];
    key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key
}

fn encode_capability_key(key: &[u8; 32]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn deny_private_chunk_payload(payload: &mut serde_json::Value) -> ExecutionError {
    super::protocol::clear_sensitive_payload_value(payload);
    ExecutionError::new(
        ExecutionErrorKind::PolicyDenied,
        "Private-file transfer is unavailable until scoped worker grants are enforced",
        false,
    )
}

pub struct RemoteWorkerClient {
    config: SshWorkerConfig,
    transport: Box<dyn RemoteRpcTransport>,
    worker_id: String,
    probe: BackendProbeV1,
    sequences: HashMap<String, u64>,
    last_health_check: Instant,
    system_image_capability_key: [u8; 32],
}

impl RemoteWorkerClient {
    pub(super) fn verified_host_key_fingerprint(&self) -> &str {
        &self.config.verified_host_key_fingerprint
    }

    #[cfg(test)]
    pub fn terminate_transport_for_test(&mut self) {
        let _ = self.transport.terminate();
    }

    pub fn connect(config: &SshWorkerConfig) -> Result<Self, ExecutionError> {
        Self::connect_inner(config, true)
    }

    /// Establish a live, identity-verified health probe without replaying any
    /// pending execution requests. Scheduler discovery must never dispatch old
    /// work as a side effect of target selection.
    pub fn connect_for_probe(config: &SshWorkerConfig) -> Result<Self, ExecutionError> {
        Self::connect_inner(config, false)
    }

    fn connect_inner(
        config: &SshWorkerConfig,
        replay_pending: bool,
    ) -> Result<Self, ExecutionError> {
        let worker = verified(config)?;
        let transport: Box<dyn RemoteRpcTransport> =
            Box::new(SshTransport::connect(&worker).map_err(|_| disconnected())?);
        let system_image_capability_key = new_capability_key();
        let mut client = Self {
            config: config.clone(),
            transport,
            worker_id: config.stable_id.clone(),
            probe: unavailable_probe(),
            sequences: HashMap::new(),
            last_health_check: Instant::now(),
            system_image_capability_key,
        };
        let ack = client
            .exchange(
                Kind::Health,
                SYSTEM_IMAGE_SESSION_ID_V1,
                serde_json::json!({
                    "wire_version": PROTOCOL_VERSION_V1,
                    "system_image_key": encode_capability_key(&client.system_image_capability_key),
                }),
            )
            .map_err(|error| client.health_error_with_diagnostics(error))?;
        client.probe = match negotiated_probe(ack.payload, &client.config.ssh_principal) {
            Ok(probe) => probe,
            Err(error) => {
                client.persist_health(
                    crate::persistence::remote_workers::RemoteWorkerHealth::Incompatible,
                );
                return Err(error);
            }
        };
        client.record_current_health();
        client.last_health_check = Instant::now();
        if replay_pending {
            client.replay_pending_requests()?;
        }
        Ok(client)
    }

    pub fn probe(&self) -> &BackendProbeV1 {
        &self.probe
    }

    pub(super) fn system_image_capability(
        &self,
        transfer_id: &str,
        proposal: &crate::execution::docker::image_extension::SystemImageExtensionRequest,
        decision: crate::harness::DependencyDecision,
        workspace: &crate::execution::contract::WorkspaceRef,
        manifest: &[crate::execution::docker::image_extension::builder::PackageArtifactIdentity],
    ) -> Result<String, ExecutionError> {
        crate::execution::docker::image_extension::sign_system_image_capability(
            &self.system_image_capability_key,
            transfer_id,
            proposal,
            decision,
            workspace,
            manifest,
        )
        .map_err(|_| incompatible())
    }

    pub(super) fn system_image_key_hex(&self) -> String {
        encode_capability_key(&self.system_image_capability_key)
    }

    pub fn request(
        &mut self,
        kind: Kind,
        execution_id: &str,
        mut payload: serde_json::Value,
    ) -> Result<EnvelopeV1, ExecutionError> {
        if kind == Kind::PrivateFileChunk {
            return Err(deny_private_chunk_payload(&mut payload));
        }
        self.refresh_health_if_due()?;
        self.exchange(kind, execution_id, payload)
    }

    /// Send one already-authorized secret chunk without creating a durable
    /// request record or attempting reconnect/replay after an ambiguous send.
    pub(crate) fn send_authorized_private_file(
        &mut self,
        execution_id: &str,
        grant: &crate::execution::remote::private_files::RemotePrivateFileGrantV1,
        relative_path: &str,
        source_file: &crate::execution::remote::private_files::AuthorizedPrivateFileBytes,
    ) -> Result<(), ExecutionError> {
        use crate::execution::remote::private_files::{
            MAX_PRIVATE_FILE_CHUNK_BYTES, PrivateFileChunkV1,
        };

        let now = crate::persistence::harness_settings::current_unix_ms();
        grant.validate_at(now).map_err(|_| incompatible())?;
        if grant.worker_id != self.config.stable_id
            || grant.worker_host_key_fingerprint != self.config.verified_host_key_fingerprint
            || grant.controller_id
                != crate::persistence::execution_records::owner_instance_id()
                    .map_err(|_| unavailable())?
        {
            return Err(incompatible());
        }
        crate::persistence::remote_private_file_grants::require_authorized(
            &crate::persistence::state_root(),
            grant,
            now,
        )
        .map_err(|_| incompatible())?;
        grant
            .validate_complete_file(relative_path, source_file.as_slice())
            .map_err(|_| incompatible())?;
        let bytes = source_file.as_slice();
        let mut offset = 0_usize;
        while offset < bytes.len() {
            let end = offset
                .saturating_add(MAX_PRIVATE_FILE_CHUNK_BYTES)
                .min(bytes.len());
            let chunk = PrivateFileChunkV1::new(
                grant.grant_id.clone(),
                relative_path.to_owned(),
                offset as u64,
                bytes[offset..end].to_vec(),
            )
            .map_err(|_| incompatible())?;
            let current_time = crate::persistence::harness_settings::current_unix_ms();
            grant
                .validate_at(current_time)
                .map_err(|_| incompatible())?;
            chunk
                .validate_for_grant(grant, current_time)
                .map_err(|_| incompatible())?;
            self.refresh_health_if_due()?;
            let _grant_use =
                crate::persistence::remote_private_file_grants::lock_authorized_grants(
                    &crate::persistence::state_root(),
                    std::slice::from_ref(grant),
                    current_time,
                )
                .map_err(|_| incompatible())?;
            self.send_private_chunk_frame(execution_id, &chunk)?;
            offset = end;
        }
        Ok(())
    }

    fn send_private_chunk_frame(
        &mut self,
        execution_id: &str,
        chunk: &crate::execution::remote::private_files::PrivateFileChunkV1,
    ) -> Result<(), ExecutionError> {
        use super::protocol::EnvelopeKindV1 as Kind;

        let sequence = self
            .sequences
            .get(execution_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        let payload = serde_json::to_value(chunk.header().map_err(|_| incompatible())?)
            .map_err(|_| incompatible())?;
        let envelope = EnvelopeV1 {
            version: PROTOCOL_VERSION_V1,
            request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            execution_id: execution_id.to_owned(),
            sequence,
            kind: Kind::PrivateFileChunk,
            payload,
        };
        envelope.validate().map_err(|_| incompatible())?;
        self.sequences.insert(execution_id.to_owned(), sequence);
        if self
            .transport
            .send_private_file_chunk(&envelope, chunk)
            .is_err()
        {
            let _ = self.transport.terminate();
            self.persist_health(
                crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
            );
            return Err(self.disconnected_with_diagnostics());
        }
        let response = match self.transport.receive() {
            Ok(Some(response)) => response,
            _ => {
                let _ = self.transport.terminate();
                self.persist_health(
                    crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
                );
                return Err(self.disconnected_with_diagnostics());
            }
        };
        if response.request_id != envelope.request_id
            || response.execution_id != envelope.execution_id
            || response.sequence != envelope.sequence
        {
            let _ = self.transport.terminate();
            return Err(incompatible());
        }
        if response.kind == Kind::Error {
            let error: ExecutionError =
                serde_json::from_value(response.payload).map_err(|_| incompatible())?;
            error.validate().map_err(|_| incompatible())?;
            return Err(error);
        }
        if response.kind != Kind::Acknowledge {
            let _ = self.transport.terminate();
            return Err(incompatible());
        }
        Ok(())
    }

    pub fn heartbeat(&mut self) -> Result<(), ExecutionError> {
        let nonce = uuid::Uuid::new_v4().to_string();
        self.exchange(
            Kind::Heartbeat,
            CONTROL_ID,
            serde_json::json!({"nonce": nonce}),
        )?;
        Ok(())
    }

    pub fn pending_request_ids(&self) -> Result<Vec<String>, ExecutionError> {
        remote_requests::pending(&self.worker_id)
            .map(|items| {
                items
                    .into_iter()
                    .map(|item| item.envelope.request_id)
                    .collect()
            })
            .map_err(|_| unavailable())
    }

    pub fn take_reconciled_response(
        &self,
        request_id: &str,
    ) -> Result<Option<EnvelopeV1>, ExecutionError> {
        remote_requests::take_completion(&self.worker_id, request_id)
            .map(|item| item.map(|record| record.response))
            .map_err(|_| unavailable())
    }
}

#[cfg(test)]
#[path = "client/reconnect_tests.rs"]
mod reconnect_tests;
