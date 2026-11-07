use ed25519_dalek::VerifyingKey;

use super::{GuestImageManifestTrustV1, ImageCacheErrorV1};

/// Stable key identity shipped in the application and used by the protected
/// catalog-promotion workflow. Rotation requires a reviewed app release.
const CATALOG_SIGNER_KEY_ID: &str = "f0d468f0-69f3-4495-887e-5b0fbb4f98f4";
const CATALOG_SIGNER_PUBLIC_KEY: [u8; 32] = [
    0x46, 0x2c, 0xf5, 0x97, 0x07, 0x7b, 0xe6, 0x4f, 0x2a, 0xae, 0x6c, 0xad, 0xf1, 0x45, 0x40, 0x8f,
    0x44, 0xe4, 0x28, 0x52, 0x08, 0xf5, 0x9a, 0xb1, 0x74, 0xa0, 0x4c, 0x36, 0x99, 0x38, 0x3d, 0x0a,
];
const MINIMUM_CATALOG_GENERATION: u64 = 1;

/// Construct trust only from the key embedded in this application release.
pub fn application_guest_image_manifest_trust_v1()
-> Result<GuestImageManifestTrustV1, ImageCacheErrorV1> {
    let key = VerifyingKey::from_bytes(&CATALOG_SIGNER_PUBLIC_KEY)
        .map_err(|_| ImageCacheErrorV1::InvalidTrustStore)?;
    GuestImageManifestTrustV1::from_application_pinned_keys(
        [(CATALOG_SIGNER_KEY_ID.to_owned(), key)],
        MINIMUM_CATALOG_GENERATION,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn application_pins_a_valid_catalog_signer() {
        assert!(super::application_guest_image_manifest_trust_v1().is_ok());
    }
}
