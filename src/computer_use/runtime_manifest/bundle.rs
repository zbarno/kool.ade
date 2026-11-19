use std::{fs, io, path::Path};

use super::{
    manifest::{MAX_RUNTIME_MANIFEST_BYTES, RuntimeManifestErrorV1, RuntimeManifestTrustV1},
    verification::{VerifiedRuntimeManifestV1, verify_runtime_manifest},
};

/// Exact file name the manifest occupies inside the bundle resource root.
/// Discovery is by fixed name only; globbing or fallback names are refused by
/// design.
pub const BUNDLED_RUNTIME_MANIFEST_FILENAME: &str = "koolade-computer-runtime-manifest-v1.json";

/// Load and verify the manifest co-located with the app's runtime resources.
///
/// A missing manifest is a deliberate `Ok(None)`: adapters without a
/// provisioned runtime must fail the same closed gate as adapters with a bad
/// one. Callers must not interpret absence as "no constraints apply".
pub fn load_bundled_runtime_manifest(
    bundle_resources: &Path,
    trust: &RuntimeManifestTrustV1,
    now_unix_ms: u64,
) -> Result<Option<VerifiedRuntimeManifestV1>, RuntimeManifestErrorV1> {
    let root = fs::canonicalize(bundle_resources).map_err(|_| RuntimeManifestErrorV1::Io)?;
    let candidate = root.join(BUNDLED_RUNTIME_MANIFEST_FILENAME);
    let metadata = match fs::symlink_metadata(&candidate) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RuntimeManifestErrorV1::Io),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RuntimeManifestErrorV1::InvalidLayout);
    }
    if metadata.len() == 0 || metadata.len() as usize > MAX_RUNTIME_MANIFEST_BYTES {
        return Err(RuntimeManifestErrorV1::TooLarge);
    }
    let bytes = fs::read(&candidate).map_err(|_| RuntimeManifestErrorV1::Io)?;
    Ok(Some(verify_runtime_manifest(&bytes, trust, now_unix_ms)?))
}
