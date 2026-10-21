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
mod private_files;
mod reconnect;
mod system_image;
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
}

#[cfg(test)]
#[path = "client/reconnect_tests.rs"]
mod reconnect_tests;
