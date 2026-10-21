use crate::{
    execution::{
        contract::HarnessRole,
        docker::{environment::DockerEnvironmentStatus, images::ImageLock},
    },
    persistence::harness_settings::{self, HarnessSettings},
    runtime_qa::RuntimeQaStore,
    ui::theme,
};

static IMAGE_LOCK: std::sync::OnceLock<Option<ImageLock>> = std::sync::OnceLock::new();

pub(super) fn paint(ui: &mut egui::Ui, docker: &DockerEnvironmentStatus) {
    ui.label(theme::section_heading("Runtime QA"));
    ui.colored_label(theme::WARNING, "Unavailable — release gates pending (#171)");
    ui.label(theme::helper_text(
        "Docker readiness does not mean browser or desktop QA is ready. QA execution stays disabled until the pinned images, isolated lifecycle, and release security checks are approved.",
    ));

    let lock = IMAGE_LOCK
        .get_or_init(|| ImageLock::bundled().ok())
        .as_ref();
    ui.label(format!(
        "Browser QA image: {}",
        profile_status(lock, "browser_qa", docker)
    ));
    ui.label(format!(
        "Linux desktop QA image and software display: {}",
        profile_status(lock, "desktop_qa", docker)
    ));
    ui.label(format!(
        "QA reviewer route: {}",
        reviewer_status(&harness_settings::load().0)
    ));
    ui.label(format!("Preview port relay: {}", relay_status(docker)));
    ui.label(theme::metadata_text(format!(
        "Evidence storage: {}",
        evidence_storage_status()
    )));
    ui.label(theme::metadata_text(
        "Per-run evidence is capped by its approved request (maximum 2 GiB); there is no automatic expiry yet. Records remain immutable in user-local storage.",
    ));
    ui.label(theme::metadata_text(
        "Role readiness uses the latest Coding Tools capability probe. Actual QA selection remains workspace and task scoped.",
    ));
}

fn profile_status(
    lock: Option<&ImageLock>,
    name: &str,
    docker: &DockerEnvironmentStatus,
) -> String {
    let Some(lock) = lock else {
        return "unavailable; bundled image lock could not be validated".into();
    };
    let Some(variants) = lock.profiles.get(name) else {
        return "unavailable; no approved pinned image is present".into();
    };
    let Some(variant) = variants.first() else {
        return "unavailable; no approved pinned image is present".into();
    };
    let digest = variant
        .digest
        .strip_prefix("sha256:")
        .unwrap_or(&variant.digest);
    let local = if docker.available_images.contains(&variant.reference) {
        "verified locally"
    } else {
        "not verified locally"
    };
    let policy = if variant.runtime_enabled {
        "enabled by image policy"
    } else {
        "disabled by reviewed image policy"
    };
    format!(
        "{policy}; {local}; linux image digest sha256:{}…",
        digest.chars().take(12).collect::<String>()
    )
}

fn reviewer_status(settings: &HarnessSettings) -> String {
    let route = settings
        .work_routes
        .get(harness_settings::QA)
        .map(|route| route.harness.as_str())
        .or(settings.default_harness.as_deref());
    let Some(id) = route else {
        return "unavailable; select a QA-capable CLI in Coding Tools".into();
    };
    let Some(detected) = settings.discovered.get(id) else {
        return format!("unverified; refresh {id} in Coding Tools");
    };
    if detected.supports_role_at(
        HarnessRole::RuntimeQaReviewer,
        harness_settings::current_unix_ms(),
    ) {
        format!("ready — {id} has a fresh Runtime QA reviewer capability")
    } else {
        format!("unavailable — {id} has no fresh Runtime QA reviewer capability")
    }
}

fn relay_status(docker: &DockerEnvironmentStatus) -> String {
    if !docker.engine_ready {
        return "unavailable; Docker is not ready".into();
    }
    "app-owned and created per QA run; preview reachability is checked when a task runs".into()
}

fn evidence_storage_status() -> String {
    let root = match RuntimeQaStore::local_storage_root() {
        Ok(root) => root,
        Err(error) => return format!("Evidence storage unavailable: {error}"),
    };
    let mut existing = root.as_path();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return format!(
                    "Evidence storage unavailable: {} is a symbolic link",
                    existing.display()
                );
            }
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return format!(
                    "Evidence storage unavailable: {} is not a directory",
                    existing.display()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = existing.parent() else {
                    return "Evidence storage unavailable: no readable data directory exists"
                        .into();
                };
                existing = parent;
            }
            Err(error) => return format!("Evidence storage unavailable: {error}"),
        }
    }
    if std::fs::read_dir(existing).is_err() {
        return format!(
            "Evidence storage unavailable: {} cannot be read",
            existing.display()
        );
    }
    let free = fs2::available_space(existing)
        .map(|bytes| format!("; {} GiB free", bytes / 1024 / 1024 / 1024))
        .unwrap_or_default();
    if existing == root {
        format!("Readable at {}{free}", root.display())
    } else {
        format!(
            "Not created yet at {}; nearest data directory {} is readable{free}",
            root.display(),
            existing.display()
        )
    }
}

#[cfg(test)]
mod tests;
