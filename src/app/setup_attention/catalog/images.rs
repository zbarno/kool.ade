use std::collections::BTreeSet;

use crate::{
    app::setup_attention::catalog::{Finding, HostKind},
    execution::docker::{
        environment::DockerEnvironmentStatus,
        images::{
            DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES, DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES,
            ImageLock, RuntimeProfile,
        },
    },
};

use super::dependencies;

const PROFILES: [(&str, RuntimeProfile); 5] = [
    ("general", RuntimeProfile::General),
    ("rust", RuntimeProfile::Rust),
    ("node", RuntimeProfile::Node),
    ("python", RuntimeProfile::Python),
    ("go", RuntimeProfile::Go),
];

pub(super) fn findings(
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    required_profiles: &BTreeSet<RuntimeProfile>,
    smoke_verified_profiles: &BTreeSet<RuntimeProfile>,
) -> Vec<Finding> {
    let Some((architecture, engine_identity, capacity_probe_required)) =
        trusted_image_engine(docker, host)
    else {
        return Vec::new();
    };
    let Ok(lock) = ImageLock::bundled() else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for (name, profile) in PROFILES {
        if capacity_probe_required && profile != RuntimeProfile::General {
            continue;
        }
        if profile != RuntimeProfile::General && !required_profiles.contains(&profile) {
            continue;
        }
        if capacity_probe_required && !bounded_desktop_general_image(&lock, architecture) {
            continue;
        }
        let id = task_id(name, architecture, &engine_identity);
        match lock.select(profile, architecture) {
            Ok(image)
                if !capacity_probe_required
                    && docker.available_images.contains(&image.reference)
                    && smoke_verified_profiles.contains(&profile) => {}
            Ok(image) => {
                let title = format!("Prepare the approved {name} runtime image");
                let explanation = if capacity_probe_required {
                    format!(
                        "The verified Docker Desktop Linux VM has no measurable free-space sample yet. The {name} image is the only bounded bootstrap image allowed; capacity must be measured before the image receives a successful preparation receipt."
                    )
                } else if docker.available_images.contains(&image.reference) {
                    format!(
                        "The pinned {name} image is present but lacks a successful smoke receipt for this Docker Engine."
                    )
                } else {
                    format!(
                        "Docker is accessible but the locked {name} runtime image has not been verified."
                    )
                };
                findings.push(dependencies::requires(
                    Finding::action(
                        id,
                        &title,
                        &explanation,
                        "Review and approve the pinned image preparation and isolation smoke test. If the exact digest is already present, no download is needed.",
                    ),
                    image_prerequisites(profile, host, capacity_probe_required),
                ));
            }
            Err(_) => findings.push(dependencies::requires(
                Finding::unverified(
                    id,
                    &format!("Verify the approved {name} runtime image"),
                    &format!(
                        "The bundled {name} image lock could not be verified for this Engine."
                    ),
                    "Refresh the application image metadata before preparing a runtime image.",
                ),
                image_prerequisites(profile, host, capacity_probe_required),
            )),
        }
    }
    findings
}

pub(super) fn checked_domains(
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    required_profiles: &BTreeSet<RuntimeProfile>,
    smoke_verified_profiles: &BTreeSet<RuntimeProfile>,
) -> Vec<String> {
    let Some((architecture, engine_identity, capacity_probe_required)) =
        trusted_image_engine(docker, host)
    else {
        return Vec::new();
    };
    if capacity_probe_required || !docker.engine_ready {
        return Vec::new();
    }
    let Ok(lock) = ImageLock::bundled() else {
        return Vec::new();
    };
    PROFILES
        .into_iter()
        .filter(|(_, profile)| {
            *profile == RuntimeProfile::General || required_profiles.contains(profile)
        })
        .filter_map(|(name, profile)| {
            let image = lock.select(profile, architecture).ok()?;
            (docker.available_images.contains(&image.reference)
                && smoke_verified_profiles.contains(&profile))
            .then(|| format!("images:{name}:{architecture}:{engine_identity}"))
        })
        .collect()
}

fn trusted_image_engine(
    docker: &DockerEnvironmentStatus,
    host: HostKind,
) -> Option<(&str, String, bool)> {
    let architecture = docker
        .architecture
        .as_deref()
        .filter(|value| matches!(*value, "amd64" | "arm64"))?;
    let capacity_probe_required = desktop_capacity_probe_required(docker, host, architecture);
    let supported_engine = match host {
        HostKind::Linux => {
            docker.engine_ready && !docker.desktop_vm && docker.rootless == Some(true)
        }
        HostKind::MacOs | HostKind::Windows => {
            docker.desktop_vm
                && docker.desktop_transport_verified
                && (capacity_probe_required
                    || (docker.engine_ready
                        && docker.desktop_vm_capacity_verified
                        && docker.workspace_staging_available
                        && docker.disk_available_bytes.is_some()))
        }
        HostKind::Unsupported => false,
    };
    if !supported_engine {
        return None;
    }
    Some((
        architecture,
        docker.engine_identity_fingerprint()?,
        capacity_probe_required,
    ))
}

fn task_id(profile: &str, architecture: &str, engine_identity: &str) -> String {
    format!("setup:images:{profile}:{architecture}:{engine_identity}")
}

fn image_prerequisites(
    profile: RuntimeProfile,
    host: HostKind,
    capacity_probe_required: bool,
) -> Vec<String> {
    if capacity_probe_required {
        // The approved, narrowly bounded General image is the step that
        // enables the VM-side capacity check. The approval flow revalidates
        // transport, image lock, host identity, and architecture itself.
        return Vec::new();
    }
    let mut prerequisites = vec![dependencies::docker_engine()];
    if host == HostKind::Linux && profile != RuntimeProfile::General {
        prerequisites.push(dependencies::docker_rootless());
    }
    prerequisites.push(dependencies::docker_capabilities());
    prerequisites
}

fn desktop_capacity_probe_required(
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    architecture: &str,
) -> bool {
    matches!(host, HostKind::MacOs | HostKind::Windows)
        && docker.desktop_vm
        && docker.desktop_transport_verified
        && docker.workspace_staging_available
        && !docker.engine_ready
        && docker.state
            == crate::execution::docker::environment::DockerEnvironmentState::InsufficientDisk
        && !docker.desktop_vm_capacity_verified
        && docker.disk_available_bytes.is_none()
        && bounded_desktop_general_image(
            &match ImageLock::bundled() {
                Ok(lock) => lock,
                Err(_) => return false,
            },
            architecture,
        )
}

fn bounded_desktop_general_image(lock: &ImageLock, architecture: &str) -> bool {
    lock.size_estimate(RuntimeProfile::General, architecture)
        .is_ok_and(|estimate| {
            estimate.estimated_pull_bytes <= DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES
                && estimate.estimated_storage_bytes <= DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES
        })
}

pub(super) fn general_smoke_verified(docker: &DockerEnvironmentStatus, architecture: &str) -> bool {
    let Some(image) = ImageLock::bundled()
        .ok()
        .and_then(|lock| lock.select(RuntimeProfile::General, architecture).ok())
    else {
        return false;
    };
    let endpoint_identity = DockerEnvironmentStatus::current_endpoint_identity_fingerprint();
    crate::persistence::linux_rootless_smoke_receipt::matches_current(
        endpoint_identity.as_deref(),
        docker.daemon_identity_fingerprint.as_deref(),
        docker.daemon_version.as_deref(),
        Some(&image),
    )
}

#[cfg(test)]
#[path = "images/tests.rs"]
mod tests;
