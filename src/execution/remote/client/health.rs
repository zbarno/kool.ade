use std::time::Instant;

use super::{CONTROL_ID, HEALTH_REFRESH_INTERVAL, RemoteWorkerClient, helpers::negotiated_probe};
use crate::execution::remote::protocol::{EnvelopeKindV1 as Kind, PROTOCOL_VERSION_V1};

impl RemoteWorkerClient {
    pub(super) fn refresh_health_if_due(
        &mut self,
    ) -> Result<(), crate::execution::errors::ExecutionError> {
        if self.last_health_check.elapsed() < HEALTH_REFRESH_INTERVAL {
            return Ok(());
        }
        let response = self.exchange(
            Kind::Health,
            CONTROL_ID,
            serde_json::json!({"wire_version": PROTOCOL_VERSION_V1}),
        )?;
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
        Ok(())
    }

    pub(super) fn persist_health(
        &self,
        health: crate::persistence::remote_workers::RemoteWorkerHealth,
    ) {
        let capabilities = self.probe.capabilities.as_ref();
        let _ = crate::persistence::remote_workers::record_probe(
            &self.worker_id,
            crate::persistence::remote_workers::RemoteWorkerProbe {
                observed_at_unix_ms: 0,
                health,
                architecture: self.probe.architecture.clone(),
                running_jobs: capabilities.map_or(0, |value| value.current_execution_count),
                policy_revision: capabilities
                    .map_or_else(String::new, |value| value.policy_hash.clone()),
                docker_version: capabilities.and_then(|value| value.docker_version.clone()),
                trusted_image_digests: capabilities
                    .map_or_else(Vec::new, |value| value.trusted_image_digests.clone()),
                resource_ceiling: capabilities.map(|value| value.resource_ceiling.clone()),
                disk_available_bytes: capabilities.and_then(|value| value.disk_available_bytes),
            },
        );
    }

    pub(super) fn record_current_health(&self) {
        let health = match crate::persistence::remote_workers::get(&self.worker_id) {
            Ok(Some(registration)) if !registration.enabled => {
                crate::persistence::remote_workers::RemoteWorkerHealth::PolicyBlocked
            }
            Ok(Some(registration))
                if self
                    .probe
                    .capabilities
                    .as_ref()
                    .is_some_and(|capabilities| {
                        capabilities.current_execution_count >= registration.max_jobs
                    }) =>
            {
                crate::persistence::remote_workers::RemoteWorkerHealth::Busy
            }
            Ok(Some(_)) => crate::persistence::remote_workers::RemoteWorkerHealth::Healthy,
            _ => crate::persistence::remote_workers::RemoteWorkerHealth::Untrusted,
        };
        self.persist_health(health);
    }
}
