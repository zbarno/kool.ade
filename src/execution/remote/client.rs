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
    protocol::{EnvelopeKindV1 as Kind, EnvelopeV1, PROTOCOL_VERSION_V1},
    ssh::SshWorkerConfig,
    transport::SshTransport,
};
mod health;
mod helpers;
mod reconnect;
pub(super) use helpers::matches_response;
use helpers::{
    disconnected, incompatible, negotiated_probe, unavailable, unavailable_probe, verified,
};

const CONTROL_ID: &str = "00000000-0000-4000-8000-000000000000";
const HEALTH_REFRESH_INTERVAL: Duration = Duration::from_secs(15);

pub struct RemoteWorkerClient {
    config: SshWorkerConfig,
    transport: SshTransport,
    worker_id: String,
    probe: BackendProbeV1,
    sequences: HashMap<String, u64>,
    last_health_check: Instant,
}

impl RemoteWorkerClient {
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
        let transport = SshTransport::connect(&worker).map_err(|_| disconnected())?;
        let mut client = Self {
            config: config.clone(),
            transport,
            worker_id: config.stable_id.clone(),
            probe: unavailable_probe(),
            sequences: HashMap::new(),
            last_health_check: Instant::now(),
        };
        let ack = client
            .exchange(
                Kind::Health,
                CONTROL_ID,
                serde_json::json!({"wire_version": PROTOCOL_VERSION_V1}),
            )
            .map_err(|error| client.health_error_with_diagnostics(error))?;
        client.probe = match negotiated_probe(ack.payload) {
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
        payload: serde_json::Value,
    ) -> Result<EnvelopeV1, ExecutionError> {
        self.refresh_health_if_due()?;
        self.exchange(kind, execution_id, payload)
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

    fn exchange(
        &mut self,
        kind: Kind,
        execution_id: &str,
        payload: serde_json::Value,
    ) -> Result<EnvelopeV1, ExecutionError> {
        let sequence = self
            .sequences
            .get(execution_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        let envelope = EnvelopeV1 {
            version: PROTOCOL_VERSION_V1,
            request_id: uuid::Uuid::new_v4().to_string(),
            execution_id: execution_id.to_owned(),
            sequence,
            kind,
            payload,
        };
        remote_requests::save_pending(&self.worker_id, &envelope).map_err(|_| unavailable())?;
        self.sequences.insert(execution_id.to_owned(), sequence);
        let response = match self.send_envelope(&envelope) {
            Ok(response) => response,
            Err(_) => {
                if self.reconnect_and_negotiate().is_err() {
                    self.persist_health(
                        crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
                    );
                    return Err(self.disconnected_with_diagnostics());
                }
                match self.send_envelope(&envelope) {
                    Ok(response) => response,
                    Err(_) => {
                        self.persist_health(
                            crate::persistence::remote_workers::RemoteWorkerHealth::Disconnected,
                        );
                        return Err(self.disconnected_with_diagnostics());
                    }
                }
            }
        };
        self.finish_response(&envelope, response)
    }

    fn send_envelope(&mut self, envelope: &EnvelopeV1) -> Result<EnvelopeV1, std::io::Error> {
        self.transport.send(envelope)?;
        let response = self.transport.receive()?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "worker closed the protocol stream",
            )
        })?;
        if response.request_id != envelope.request_id
            || response.execution_id != envelope.execution_id
            || response.sequence != envelope.sequence
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "worker response identity does not match request",
            ));
        }
        if response.kind != Kind::Error && !matches_response(&envelope.kind, &response.kind) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "worker response kind does not match request",
            ));
        }
        Ok(response)
    }

    fn finish_response(
        &self,
        request: &EnvelopeV1,
        response: EnvelopeV1,
    ) -> Result<EnvelopeV1, ExecutionError> {
        if response.kind == Kind::Error {
            let error: ExecutionError =
                serde_json::from_value(response.payload.clone()).map_err(|parse_error| {
                    ExecutionError::new(
                        ExecutionErrorKind::WorkerIncompatible,
                        format!("Remote worker returned an invalid error response: {parse_error}"),
                        false,
                    )
                })?;
            error.validate().map_err(|validation_error| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerIncompatible,
                    format!("Remote worker returned an invalid error response: {validation_error}"),
                    false,
                )
            })?;
            if error.kind != ExecutionErrorKind::WorkerDisconnected {
                remote_requests::record_completion(&self.worker_id, &request.request_id, &response)
                    .map_err(|_| unavailable())?;
            }
            return Err(error);
        }
        remote_requests::record_completion(&self.worker_id, &request.request_id, &response)
            .map_err(|_| unavailable())?;
        Ok(response)
    }

    fn health_error_with_diagnostics(&self, error: ExecutionError) -> ExecutionError {
        let mut diagnostic = self
            .transport
            .diagnostic_stderr()
            .chars()
            .filter(|character| !character.is_control())
            .take(512)
            .collect::<String>();
        while diagnostic.len() > 512 {
            diagnostic.pop();
        }
        if diagnostic.is_empty() {
            return error;
        }
        let mut message = error.message;
        while message.len() > 3_500 {
            message.pop();
        }
        ExecutionError::new(
            error.kind,
            format!("{message}; SSH worker diagnostic: {diagnostic}"),
            error.retryable,
        )
    }

    fn reconnect_and_negotiate(&mut self) -> Result<(), ExecutionError> {
        let transport =
            SshTransport::connect(&verified(&self.config)?).map_err(|_| disconnected())?;
        self.transport = transport;
        let request = EnvelopeV1 {
            version: PROTOCOL_VERSION_V1,
            request_id: uuid::Uuid::new_v4().to_string(),
            execution_id: CONTROL_ID.to_owned(),
            sequence: 1,
            kind: Kind::Health,
            payload: serde_json::json!({"wire_version": PROTOCOL_VERSION_V1}),
        };
        self.transport.send(&request).map_err(|_| disconnected())?;
        let response = self
            .transport
            .receive()
            .map_err(|_| disconnected())?
            .ok_or_else(disconnected)?;
        if response.request_id != request.request_id
            || response.execution_id != request.execution_id
            || response.sequence != request.sequence
        {
            return Err(incompatible());
        }
        if response.kind == Kind::Error {
            let error: ExecutionError =
                serde_json::from_value(response.payload.clone()).map_err(|_| incompatible())?;
            error.validate().map_err(|_| incompatible())?;
            return Err(error);
        }
        if response.kind != Kind::Acknowledge {
            return Err(incompatible());
        }
        self.probe = match negotiated_probe(response.payload) {
            Ok(probe) => probe,
            Err(error) => {
                self.persist_health(
                    crate::persistence::remote_workers::RemoteWorkerHealth::Incompatible,
                );
                return Err(error);
            }
        };
        self.record_current_health();
        self.last_health_check = Instant::now();
        // Keep the health handshake out of the session that replays the exact
        // durable request. A control request may itself have sequence 1.
        self.transport =
            SshTransport::connect(&verified(&self.config)?).map_err(|_| disconnected())?;
        Ok(())
    }
}
