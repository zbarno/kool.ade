use std::collections::BTreeSet;

use crate::{
    app::setup_attention::catalog::{self, HostKind},
    execution::docker::{environment::DockerEnvironmentStatus, images::RuntimeProfile},
    persistence::setup_tasks::SetupTaskStoreV1,
};

const PROFILES: [(&str, RuntimeProfile); 5] = [
    ("general", RuntimeProfile::General),
    ("rust", RuntimeProfile::Rust),
    ("node", RuntimeProfile::Node),
    ("python", RuntimeProfile::Python),
    ("go", RuntimeProfile::Go),
];

pub(super) fn scan_state(
    store: &SetupTaskStoreV1,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
) -> catalog::ImageScanState {
    let required_profiles = store.required_image_profiles.clone();
    let mut smoke_verified_profiles = BTreeSet::new();
    if host == HostKind::Linux
        && docker.engine_ready
        && !docker.desktop_vm
        && docker.rootless == Some(true)
        && let (Some(architecture), Some(engine_identity)) = (
            docker
                .architecture
                .as_deref()
                .filter(|value| matches!(*value, "amd64" | "arm64")),
            docker.engine_identity_fingerprint(),
        )
    {
        for (name, profile) in PROFILES {
            if profile != RuntimeProfile::General && !required_profiles.contains(&profile) {
                continue;
            }
            let task_id = format!("setup:images:{name}:{architecture}:{engine_identity}");
            if store.has_valid_image_preparation_receipt(&task_id) {
                smoke_verified_profiles.insert(profile);
            }
        }
    }
    catalog::ImageScanState {
        required_profiles,
        smoke_verified_profiles,
    }
}
