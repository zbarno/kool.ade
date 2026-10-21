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
        self.refresh_probe_now()
    }

    pub fn refresh_probe_now(&mut self) -> Result<(), crate::execution::errors::ExecutionError> {
        let response = self.exchange(
            Kind::Health,
            CONTROL_ID,
            serde_json::json!({
                "wire_version": PROTOCOL_VERSION_V1,
                "system_image_key": self.system_image_key_hex(),
            }),
        )?;
        self.probe = match negotiated_probe(response.payload, &self.config.ssh_principal) {
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
        let _ = crate::persistence::remote_workers::record_probe(
            &self.worker_id,
            persistable_probe(&self.probe, health),
        );
    }

    pub(super) fn record_current_health(&self) {
        let health = match crate::persistence::remote_workers::get(&self.worker_id) {
            Ok(Some(registration)) => classify_health(
                registration.enabled,
                self.probe.capabilities.as_ref().map(|capabilities| {
                    (
                        capabilities.current_execution_count,
                        capabilities.docker_rootless,
                    )
                }),
                registration.max_jobs,
            ),
            _ => crate::persistence::remote_workers::RemoteWorkerHealth::Untrusted,
        };
        self.persist_health(health);
    }
}

fn persistable_probe(
    probe: &crate::execution::backend::BackendProbeV1,
    health: crate::persistence::remote_workers::RemoteWorkerHealth,
) -> crate::persistence::remote_workers::RemoteWorkerProbe {
    let capabilities = probe.capabilities.as_ref();
    crate::persistence::remote_workers::RemoteWorkerProbe {
        observed_at_unix_ms: 0,
        health,
        architecture: probe.architecture.clone(),
        running_jobs: capabilities.map_or(0, |value| value.current_execution_count),
        policy_revision: capabilities.map_or_else(String::new, |value| value.policy_hash.clone()),
        docker_version: capabilities.and_then(|value| value.docker_version.clone()),
        trusted_image_digests: capabilities
            .map_or_else(Vec::new, |value| value.trusted_image_digests.clone()),
        resource_ceiling: capabilities.map(|value| value.resource_ceiling.clone()),
        disk_available_bytes: capabilities.and_then(|value| value.disk_available_bytes),
        available_cpu_millis: capabilities.and_then(|value| value.available_cpu_millis),
        available_memory_bytes: capabilities.and_then(|value| value.available_memory_bytes),
        cpu_pressure_some_avg10_basis_points: capabilities
            .and_then(|value| value.cpu_pressure_some_avg10_basis_points),
        load_millis: capabilities.and_then(|value| value.load_millis),
        gpu_available: capabilities.map(|value| value.gpu_available),
        display_available: capabilities.map(|value| value.display_available),
    }
}

fn classify_health(
    enabled: bool,
    observation: Option<(u32, Option<bool>)>,
    max_jobs: u32,
) -> crate::persistence::remote_workers::RemoteWorkerHealth {
    use crate::persistence::remote_workers::RemoteWorkerHealth as Health;
    if !enabled {
        return Health::PolicyBlocked;
    }
    let Some((running_jobs, rootless)) = observation else {
        return Health::Incompatible;
    };
    match rootless {
        Some(false) => Health::PolicyBlocked,
        None => Health::Incompatible,
        Some(true) if running_jobs >= max_jobs => Health::Busy,
        Some(true) => Health::Healthy,
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_health, persistable_probe};
    use crate::execution::{
        backend::{BackendCapabilitiesV1, BackendProbeV1},
        contract::ResourceBudgetV1,
    };
    use crate::persistence::remote_workers::RemoteWorkerHealth as Health;

    #[test]
    fn rootful_or_unreported_worker_is_not_reported_healthy() {
        assert_eq!(
            classify_health(true, Some((0, Some(false))), 2),
            Health::PolicyBlocked
        );
        assert_eq!(
            classify_health(true, Some((0, None)), 2),
            Health::Incompatible
        );
        assert_eq!(
            classify_health(false, Some((0, Some(true))), 2),
            Health::PolicyBlocked
        );
        assert_eq!(
            classify_health(true, Some((0, Some(true))), 2),
            Health::Healthy
        );
    }

    #[test]
    fn health_probe_persists_gpu_and_isolated_display_capabilities() {
        let probe = BackendProbeV1 {
            wire_version: 1,
            available: true,
            compatible: true,
            architecture: Some("amd64".into()),
            failure: None,
            capabilities: Some(BackendCapabilitiesV1 {
                app_version: "test".into(),
                docker_version: Some("test".into()),
                docker_api_version: None,
                docker_rootless: Some(true),
                docker_desktop: None,
                linux_containers: Some(true),
                seccomp_enabled: Some(true),
                memory_limit: Some(true),
                cpu_quota: Some(true),
                pids_limit: Some(true),
                disk_available_bytes: Some(1_000_000),
                available_cpu_millis: Some(2_000),
                available_memory_bytes: Some(2_000_000),
                cpu_pressure_some_avg10_basis_points: Some(0),
                load_millis: Some(0),
                gpu_available: true,
                display_available: true,
                current_execution_count: 0,
                maximum_execution_count: Some(1),
                resource_ceiling: ResourceBudgetV1 {
                    cpu_millis: 1_000,
                    memory_bytes: 1_000_000,
                    process_count: 64,
                    temporary_disk_bytes: 1_000_000,
                    output_bytes: 1_000_000,
                },
                trusted_image_digests: vec!["sha256:trusted".into()],
                policy_hash: "sha256:policy".into(),
            }),
        };

        let persisted = persistable_probe(&probe, Health::Healthy);
        assert_eq!(persisted.gpu_available, Some(true));
        assert_eq!(persisted.display_available, Some(true));
        assert_eq!(persisted.architecture.as_deref(), Some("amd64"));
        assert_eq!(persisted.policy_revision, "sha256:policy");
    }
}
