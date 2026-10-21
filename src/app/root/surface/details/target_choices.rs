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
        crate::persistence::harness_settings::current_unix_ms(),
    )
}

fn choices(
    selected: &TargetPreference,
    settings_error: Option<&str>,
    local: &DockerEnvironmentStatus,
    workers: &[remote_workers::RemoteWorkerRegistration],
    worker_error: bool,
    now_unix_ms: u64,
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
        let probe = worker.last_probe.as_ref();
        let probe_health = probe.map(|probe| &probe.health);
        let stale_probe = probe.is_some_and(|probe| probe_is_stale(probe, now_unix_ms));
        let missing_resources = probe.is_some_and(|probe| {
            probe.available_cpu_millis.is_none()
                || probe.available_memory_bytes.is_none()
                || probe.load_millis.is_none()
        });
        let available = worker.enabled
            && !stale_probe
            && !missing_resources
            && probe_health == Some(&RemoteWorkerHealth::Healthy);
        let reason = if !worker.enabled {
            "Worker is disabled in Execution Environments.".into()
        } else if stale_probe {
            "Worker health probe is stale; reconnect before scheduling.".into()
        } else if missing_resources {
            "Worker does not report live CPU, memory, and load; update the worker before scheduling.".into()
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
            label: format!(
                "{}@{} · {}",
                worker.ssh_principal, worker.ssh_host_alias, worker.worker_id
            ),
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

fn probe_is_stale(probe: &remote_workers::RemoteWorkerProbe, now_unix_ms: u64) -> bool {
    probe.observed_at_unix_ms == 0
        || probe.observed_at_unix_ms > now_unix_ms
        || now_unix_ms.saturating_sub(probe.observed_at_unix_ms)
            > crate::execution::remote::scheduler::MAX_HEALTH_AGE_MS
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
                available_cpu_millis: Some(1_000),
                available_memory_bytes: Some(1024 * 1024 * 1024),
                cpu_pressure_some_avg10_basis_points: Some(0),
                load_millis: Some(0),
                gpu_available: Some(false),
                display_available: Some(false),
            }),
            policy_revision: "v1".into(),
        }
    }

    #[test]
    fn selected_worker_missing_from_enrollment_is_visible_but_unavailable() {
        let selected = TargetPreference::Worker("worker-no-longer-enrolled".into());
        let choices = choices(&selected, None, &local_status(), &[], false, 1);
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
        let choices = choices(&selected, None, &local_status(), &workers, false, 1);
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

    #[test]
    fn stale_or_future_healthy_probe_is_unavailable_for_target_selection() {
        let selected = TargetPreference::Auto;
        let stale_worker = worker(true, RemoteWorkerHealth::Healthy);
        let stale_choices = choices(
            &selected,
            None,
            &local_status(),
            &[stale_worker],
            false,
            crate::execution::remote::scheduler::MAX_HEALTH_AGE_MS + 2,
        );
        let choice = stale_choices
            .iter()
            .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
            .expect("enrolled worker is shown");
        assert!(!choice.available);
        assert!(choice.reason.contains("probe is stale"));

        let boundary_worker = worker(true, RemoteWorkerHealth::Healthy);
        let boundary_choices = choices(
            &selected,
            None,
            &local_status(),
            &[boundary_worker],
            false,
            crate::execution::remote::scheduler::MAX_HEALTH_AGE_MS + 1,
        );
        assert!(boundary_choices.iter().any(|choice| {
            choice.preference == TargetPreference::Worker("healthy-worker".into())
                && choice.available
        }));

        let future_probe = worker(true, RemoteWorkerHealth::Healthy);
        let future_choices = choices(&selected, None, &local_status(), &[future_probe], false, 0);
        let future_choice = future_choices
            .iter()
            .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
            .expect("enrolled worker is shown");
        assert!(!future_choice.available);
        assert!(future_choice.reason.contains("probe is stale"));

        let mut zero_probe = worker(true, RemoteWorkerHealth::Healthy);
        zero_probe
            .last_probe
            .as_mut()
            .expect("fixture has a probe")
            .observed_at_unix_ms = 0;
        let zero_choices = choices(&selected, None, &local_status(), &[zero_probe], false, 0);
        let zero_choice = zero_choices
            .iter()
            .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
            .expect("enrolled worker is shown");
        assert!(!zero_choice.available);
        assert!(zero_choice.reason.contains("probe is stale"));
    }

    #[test]
    fn healthy_worker_without_current_resource_measurements_is_unavailable() {
        let mut worker = worker(true, RemoteWorkerHealth::Healthy);
        let probe = worker.last_probe.as_mut().unwrap();
        probe.available_memory_bytes = None;
        let choices = choices(
            &TargetPreference::Auto,
            None,
            &local_status(),
            &[worker],
            false,
            2,
        );
        let choice = choices
            .iter()
            .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
            .unwrap();
        assert!(!choice.available);
        assert!(choice.reason.contains("live CPU, memory, and load"));
    }
}
