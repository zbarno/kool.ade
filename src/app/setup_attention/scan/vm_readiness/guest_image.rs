use serde::Deserialize;

use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceState, HostKind, ImageArchitectureV1,
};

pub(super) fn ready_for_host(
    host: HostKind,
    architecture: ImageArchitectureV1,
    state: EvidenceState,
) -> bool {
    host == HostKind::Linux
        && architecture == ImageArchitectureV1::Amd64
        && state == EvidenceState::Ready
}

const IMAGE_LOCK: &str = include_str!("../../../../../containers/computer-use/images.lock.json");
const SIGNED_CATALOG: &[u8] =
    include_bytes!("../../../../../containers/computer-use/catalog-v1.json");
const MAX_IMAGE_LOCK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GuestImageCatalogProbe {
    pub state: EvidenceState,
    pub code: EvidenceCode,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GuestImageBuildLockV1 {
    schema_version: u8,
    status: String,
    manifest_sha256: Option<String>,
    catalog_generation: Option<u64>,
    images: Vec<String>,
}

pub(super) fn probe() -> GuestImageCatalogProbe {
    let root = crate::persistence::setup_tasks::computer_use_storage_dir().ok();
    probe_bundle_at(IMAGE_LOCK.as_bytes(), SIGNED_CATALOG, root.as_deref())
}

fn probe_bundle_at(
    lock_bytes: &[u8],
    catalog_bytes: &[u8],
    storage_root: Option<&std::path::Path>,
) -> GuestImageCatalogProbe {
    let trust = match crate::computer_use::guest_image::application_guest_image_manifest_trust_v1()
    {
        Ok(trust) => trust,
        Err(_) => return unverified(),
    };
    probe_bundle_with_trust(lock_bytes, catalog_bytes, storage_root, &trust)
}

fn probe_bundle_with_trust(
    lock_bytes: &[u8],
    catalog_bytes: &[u8],
    storage_root: Option<&std::path::Path>,
    trust: &crate::computer_use::guest_image::GuestImageManifestTrustV1,
) -> GuestImageCatalogProbe {
    let parsed = (lock_bytes.len() <= MAX_IMAGE_LOCK_BYTES)
        .then(|| serde_json::from_slice::<GuestImageBuildLockV1>(lock_bytes).ok())
        .flatten();
    let Some(lock) = parsed else {
        return unverified();
    };
    if lock.schema_version != 1 {
        return unverified();
    }
    if lock.status == "not_promoted"
        && lock.manifest_sha256.is_none()
        && lock.catalog_generation.is_none()
        && lock.images.is_empty()
    {
        return missing();
    }
    if lock.status != "promoted"
        || lock.manifest_sha256.is_none()
        || lock.catalog_generation.is_none()
        || lock.images.is_empty()
        || lock.images.len() > 16
        || lock.images.iter().any(|digest| !is_sha256_digest(digest))
    {
        return unverified();
    }

    #[cfg(target_arch = "x86_64")]
    {
        let Some(root) = storage_root else {
            return unverified();
        };
        let floor = match crate::computer_use::guest_image::GuestImageCatalogGenerationFloorV1::open(
            root.join("catalog-floor"),
        ) {
            Ok(floor) => floor,
            Err(_) => return unverified(),
        };
        let cache = match crate::computer_use::guest_image::GuestImageCacheV1::open_existing(
            root.join("images"),
        ) {
            Ok(cache) => cache,
            Err(_) => return unverified(),
        };
        let cached = match cache.verify_cached_catalog_image(
            catalog_bytes,
            trust,
            &floor,
            crate::computer_use::guest_image::ComputerGuestArchitectureV1::X86_64,
        ) {
            Ok(cached) => cached,
            Err(_) => return unverified(),
        };
        if Some(cached.manifest_digest()) != lock.manifest_sha256.as_deref()
            || Some(cached.catalog_generation()) != lock.catalog_generation
            || !lock
                .images
                .iter()
                .any(|digest| digest == &cached.entry().image_digest)
        {
            return unverified();
        }
        GuestImageCatalogProbe {
            state: EvidenceState::Ready,
            code: EvidenceCode::ComputerGuestImageCatalogVerified,
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = storage_root;
        let _ = catalog_bytes;
        unverified()
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn missing() -> GuestImageCatalogProbe {
    GuestImageCatalogProbe {
        state: EvidenceState::Missing,
        code: EvidenceCode::ComputerGuestImageNotPromoted,
    }
}

fn unverified() -> GuestImageCatalogProbe {
    GuestImageCatalogProbe {
        state: EvidenceState::Unknown,
        code: EvidenceCode::ComputerGuestImageCatalogUnverified,
    }
}

#[cfg(test)]
#[path = "guest_image/tests.rs"]
mod tests;
