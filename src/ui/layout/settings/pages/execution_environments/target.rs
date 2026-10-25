use crate::{
    execution::{contract::TargetPreference, docker::environment::DockerEnvironmentStatus},
    persistence::remote_workers,
};

use super::state_label;

pub(super) fn label(
    target: &TargetPreference,
    workers: &[remote_workers::RemoteWorkerRegistration],
) -> String {
    match target {
        TargetPreference::Auto => "Automatic (trusted only)".into(),
        TargetPreference::Local => "Local Docker".into(),
        TargetPreference::Worker(id) => {
            let suffix = workers
                .iter()
                .find(|worker| worker.worker_id == *id)
                .map(|worker| {
                    if worker.enabled {
                        "registered"
                    } else {
                        "disabled"
                    }
                })
                .unwrap_or("unavailable");
            format!("Specific worker: {id} ({suffix})")
        }
    }
}

pub(super) fn status(
    target: &TargetPreference,
    local: &DockerEnvironmentStatus,
    workers: &[remote_workers::RemoteWorkerRegistration],
) -> String {
    match target {
        TargetPreference::Auto => format!(
            "Automatic selection uses only eligible trusted workers; local Docker is {}.",
            state_label(local.state)
        ),
        TargetPreference::Local if local.local_execution_ready()
            && local.state == crate::execution::docker::environment::DockerEnvironmentState::Ready =>
        {
            "Selected target: local Docker is ready.".into()
        }
        TargetPreference::Local if local.local_execution_ready() => "Docker is usable; an approved image will be pulled from the bundled digest lock when needed.".into(),
        TargetPreference::Local if local.ready() => "The Docker engine is reachable, but local task execution is enabled only for verified native Linux rootless Engine.".into(),
        TargetPreference::Local => format!("Selected target is unavailable: {}.", local.message),
        TargetPreference::Worker(id) => workers
            .iter()
            .find(|worker| worker.worker_id == *id)
            .map(|worker| {
                format!(
                    "Selected worker {} is {} and last reported {:?}.",
                    id,
                    if worker.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    worker.last_probe.as_ref().map(|probe| &probe.health)
                )
            })
            .unwrap_or_else(|| {
                format!("Selected worker {id} is not registered and cannot be used.")
            }),
    }
}
