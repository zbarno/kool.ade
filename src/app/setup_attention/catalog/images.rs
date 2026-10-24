use crate::{
    app::setup_attention::catalog::{Finding, HostKind},
    execution::docker::{
        environment::DockerEnvironmentStatus,
        images::{ImageLock, RuntimeProfile},
    },
};

pub(super) fn findings(docker: &DockerEnvironmentStatus, host: HostKind) -> Vec<Finding> {
    if host != HostKind::Linux
        || !docker.engine_ready
        || docker.desktop_vm
        || docker.rootless != Some(true)
    {
        return Vec::new();
    }
    let Some(architecture) = docker
        .architecture
        .as_deref()
        .filter(|value| matches!(*value, "amd64" | "arm64"))
    else {
        return Vec::new();
    };
    match ImageLock::bundled().and_then(|lock| lock.select(RuntimeProfile::General, architecture)) {
        Ok(image) if !docker.available_images.contains(&image.reference) => vec![Finding::action(
            format!("setup:images:general:{architecture}"),
            "Prepare the approved runtime image",
            "Docker is accessible but the locked general runtime image has not been verified.",
            "Review and approve the pinned image download and isolation smoke test.",
        )],
        Ok(_) => Vec::new(),
        Err(_) => vec![Finding::unverified(
            "setup:images:general:unverified",
            "Verify the approved runtime image",
            "The bundled general image lock could not be verified for this Engine.",
            "Refresh the application image metadata before preparing a runtime image.",
        )],
    }
}

pub(super) fn checked_domains(docker: &DockerEnvironmentStatus, host: HostKind) -> Vec<String> {
    if host != HostKind::Linux
        || !docker.engine_ready
        || docker.desktop_vm
        || docker.rootless != Some(true)
    {
        return Vec::new();
    }
    let Some(architecture) = docker
        .architecture
        .as_deref()
        .filter(|value| matches!(*value, "amd64" | "arm64"))
    else {
        return Vec::new();
    };
    let Ok(lock) = ImageLock::bundled() else {
        return Vec::new();
    };
    [
        ("general", RuntimeProfile::General),
        ("rust", RuntimeProfile::Rust),
        ("node", RuntimeProfile::Node),
        ("python", RuntimeProfile::Python),
        ("go", RuntimeProfile::Go),
    ]
    .into_iter()
    .filter_map(|(name, profile)| {
        let image = lock.select(profile, architecture).ok()?;
        docker
            .available_images
            .contains(&image.reference)
            .then(|| format!("images:{name}:{architecture}"))
    })
    .collect()
}
