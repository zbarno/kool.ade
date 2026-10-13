use crate::{
    execution::{contract::TargetPreference, docker::environment::DockerEnvironmentStatus},
    persistence::{remote_workers, remote_workers::RemoteWorkerHealth},
    ui::task_detail::TargetChoice,
};

pub(super) fn build(
    selected: &TargetPreference,
    settings_error: Option<&str>,
) -> Vec<TargetChoice> {
    let local = DockerEnvironmentStatus::detect();
    let workers = remote_workers::list();
    choices(
        selected,
        settings_error,
        &local,
        workers.as_deref().unwrap_or(&[]),
        workers.is_err(),
    )
}

fn choices(
    selected: &TargetPreference,
    settings_error: Option<&str>,
    local: &DockerEnvironmentStatus,
    workers: &[remote_workers::RemoteWorkerRegistration],
    worker_error: bool,
) -> Vec<TargetChoice> {
    let mut choices = vec![TargetChoice {
        preference: TargetPreference::Auto,
        label: "Automatic (trusted only)".into(),
        available: true,
        reason: settings_error.map_or_else(
            || "Uses only an explicitly trusted, enabled target that meets task requirements. No eligible target means the task stays blocked; it never falls back to host execution.".into(),
            |error| format!("Stored target settings could not be read; defaults are shown. {error} Automatic routing still requires an explicitly trusted, enabled target, and never falls back to host execution."),
        ),
    }, TargetChoice {
        preference: TargetPreference::Local,
        label: "Local Docker".into(),
        available: local.ready(),
        reason: if local.ready() {
            "Local Docker passed the current readiness check.".into()
        } else {
            format!("Local Docker is unavailable: {}", local.message)
        },
    }];
    for worker in workers {
        let probe_health = worker.last_probe.as_ref().map(|probe| &probe.health);
        let available = worker.enabled && probe_health == Some(&RemoteWorkerHealth::Healthy);
        let reason = if !worker.enabled {
            "Worker is disabled in Execution Environments.".into()
        } else {
            match probe_health {
                Some(RemoteWorkerHealth::Healthy) => {
                    "Enrolled worker is enabled and healthy.".into()
                }
                Some(health) => format!("Worker is unavailable: {}.", health_label(health)),
                None => "Worker has no successful connection probe yet.".into(),
            }
        };
        choices.push(TargetChoice {
            preference: TargetPreference::Worker(worker.worker_id.clone()),
            label: format!("Specific remote worker · {}", worker.worker_id),
            available,
            reason,
        });
    }
    if let TargetPreference::Worker(worker_id) = selected
        && !choices.iter().any(|choice| choice.preference == *selected)
    {
        choices.push(TargetChoice {
            preference: selected.clone(),
            label: format!("Specific remote worker · {worker_id}"),
            available: false,
            reason: if worker_error {
                "Could not verify this worker's enrollment. Execution remains blocked until its status can be checked.".into()
            } else {
                "This worker is no longer enrolled on this device. Select an available target explicitly; execution will not fall back.".into()
            },
        });
    }
    choices
}

fn health_label(health: &RemoteWorkerHealth) -> &'static str {
    match health {
        RemoteWorkerHealth::Healthy => "healthy",
        RemoteWorkerHealth::Busy => "busy",
        RemoteWorkerHealth::Disconnected => "disconnected",
        RemoteWorkerHealth::Untrusted => "untrusted",
        RemoteWorkerHealth::Incompatible => "incompatible",
        RemoteWorkerHealth::PolicyBlocked => "blocked by worker policy",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_status() -> DockerEnvironmentStatus {
        DockerEnvironmentStatus {
            state: crate::execution::docker::environment::DockerEnvironmentState::NotInstalled,
            engine_ready: false,
            message: "Docker is not installed".into(),
            desktop_vm: false,
            daemon_version: None,
            architecture: None,
            rootless: None,
            cpu_limits: None,
            memory_limits: None,
            pids_limit: None,
            available_images: Vec::new(),
            disk_available_bytes: None,
        }
    }

    fn worker(
        enabled: bool,
        health: RemoteWorkerHealth,
    ) -> remote_workers::RemoteWorkerRegistration {
        remote_workers::RemoteWorkerRegistration {
            schema_version: 1,
            worker_id: if enabled {
                "healthy-worker"
            } else {
                "disabled-worker"
            }
            .into(),
            ssh_host_alias: "worker-host".into(),
            ssh_principal: "developer".into(),
            host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
            enrolled_at_unix_ms: 1,
            enabled,
            max_jobs: 1,
            supported_architectures: vec!["amd64".into()],
            trusted_image_digests: Vec::new(),
            resource_limit: None,
            last_probe: Some(remote_workers::RemoteWorkerProbe {
                observed_at_unix_ms: 1,
                health,
                architecture: Some("amd64".into()),
                running_jobs: 0,
                policy_revision: "v1".into(),
                docker_version: None,
                trusted_image_digests: Vec::new(),
                resource_ceiling: None,
                disk_available_bytes: None,
            }),
            policy_revision: "v1".into(),
        }
    }

    #[test]
    fn selected_worker_missing_from_enrollment_is_visible_but_unavailable() {
        let selected = TargetPreference::Worker("worker-no-longer-enrolled".into());
        let choices = choices(&selected, None, &local_status(), &[], false);
        let choice = choices
            .iter()
            .find(|choice| choice.preference == selected)
            .expect("persisted selection remains visible");
        assert!(!choice.available);
        assert!(choice.reason.contains("no longer enrolled"));
        assert!(choices.iter().any(|choice| {
            choice.preference == TargetPreference::Auto
                && choice.label == "Automatic (trusted only)"
        }));
    }

    #[test]
    fn disabled_and_unhealthy_workers_are_explained_and_unavailable() {
        let selected = TargetPreference::Auto;
        let workers = [
            worker(false, RemoteWorkerHealth::Healthy),
            worker(true, RemoteWorkerHealth::Disconnected),
        ];
        let choices = choices(&selected, None, &local_status(), &workers, false);
        assert!(choices.iter().any(|choice| {
            choice.label.contains("disabled-worker")
                && !choice.available
                && choice.reason.contains("disabled")
        }));
        assert!(choices.iter().any(|choice| {
            choice.label.contains("healthy-worker")
                && !choice.available
                && choice.reason.contains("disconnected")
        }));
        assert!(choices.iter().any(|choice| {
            choice.preference == TargetPreference::Local
                && !choice.available
                && choice.reason.contains("Docker is not installed")
        }));
    }
}
