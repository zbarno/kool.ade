use crate::{
    execution::{contract::TargetPreference, docker::environment::DockerEnvironmentStatus},
    persistence::{remote_workers, remote_workers::RemoteWorkerHealth},
    ui::task_detail::TargetChoice,
};

mod task_requirements;

pub(super) fn apply_task_requirements(
    choices: &mut [crate::ui::task_detail::TargetChoice],
    requirements: crate::persistence::execution_environment_settings::TaskCapabilityRequirements,
) {
    task_requirements::apply_task_requirements(choices, requirements);
}

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
#[path = "target_choices/tests.rs"]
mod tests;
