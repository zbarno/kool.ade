use std::collections::BTreeSet;

use crate::{
    app::setup_attention::catalog::{Finding, HostKind},
    execution::docker::{
        environment::DockerEnvironmentStatus,
        images::{ImageLock, RuntimeProfile},
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
    let Some((architecture, engine_identity)) = trusted_image_engine(docker, host) else {
        return Vec::new();
    };
    let Ok(lock) = ImageLock::bundled() else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for (name, profile) in PROFILES {
        if profile != RuntimeProfile::General && !required_profiles.contains(&profile) {
            continue;
        }
        let id = task_id(name, architecture, &engine_identity);
        match lock.select(profile, architecture) {
            Ok(image)
                if docker.available_images.contains(&image.reference)
                    && smoke_verified_profiles.contains(&profile) => {}
            Ok(image) => {
                let title = format!("Prepare the approved {name} runtime image");
                let explanation = if docker.available_images.contains(&image.reference) {
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
                    [
                        dependencies::docker_engine(),
                        dependencies::docker_rootless(),
                        dependencies::docker_capabilities(),
                    ],
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
                [
                    dependencies::docker_engine(),
                    dependencies::docker_rootless(),
                    dependencies::docker_capabilities(),
                ],
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
    let Some((architecture, engine_identity)) = trusted_image_engine(docker, host) else {
        return Vec::new();
    };
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
) -> Option<(&str, String)> {
    if host != HostKind::Linux
        || !docker.engine_ready
        || docker.desktop_vm
        || docker.rootless != Some(true)
    {
        return None;
    }
    let architecture = docker
        .architecture
        .as_deref()
        .filter(|value| matches!(*value, "amd64" | "arm64"))?;
    Some((architecture, docker.engine_identity_fingerprint()?))
}

fn task_id(profile: &str, architecture: &str, engine_identity: &str) -> String {
    format!("setup:images:{profile}:{architecture}:{engine_identity}")
}
