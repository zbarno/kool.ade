use super::manifest::{RuntimeManifestErrorV1, RuntimeManifestTrustV1};
use crate::computer_use::guest_image::application_catalog_signer;

/// Role-separated identity for the runtime-manifest signer.
///
/// The key material is, for now, the same release Ed25519 key that signs the
/// guest image catalog. Roles are separated by distinct signature domains
/// (`koolade.computer-runtime-manifest.v1` versus
/// `koolade.computer-image-catalog.v1`) and a distinct key id. Provisioning a
/// dedicated runtime key — like the image release key before it — requires a
/// reviewed app release.
const RUNTIME_SIGNER_KEY_ID: &str = "9b1c6e04-7a2f-4f3d-bc5b-2d4e6f8a0c1e";

const MINIMUM_RUNTIME_MANIFEST_GENERATION: u64 = 1;

/// Construct trust only from the key embedded in this application release.
pub fn application_runtime_manifest_trust_v1()
-> Result<RuntimeManifestTrustV1, RuntimeManifestErrorV1> {
    let (_, key) =
        application_catalog_signer().map_err(|_| RuntimeManifestErrorV1::UntrustedTrustStore)?;
    RuntimeManifestTrustV1::from_application_pinned_keys(
        [(RUNTIME_SIGNER_KEY_ID.to_owned(), key)],
        MINIMUM_RUNTIME_MANIFEST_GENERATION,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn application_pins_a_valid_runtime_signer_role() {
        let trust = super::application_runtime_manifest_trust_v1().expect("pinned trust");
        assert_eq!(trust.minimum_generation(), 1);
    }
}
