use ed25519_dalek::VerifyingKey;

use super::{GuestImageManifestTrustV1, ImageCacheErrorV1};

/// Stable key identity shipped in the application and used by the protected
/// catalog-promotion workflow. Rotation requires a reviewed app release.
const CATALOG_SIGNER_KEY_ID: &str = "76fe57c9-0102-424b-9bb2-90b7ca745006";
const CATALOG_SIGNER_PUBLIC_KEY: [u8; 32] = [
    0xfc, 0xef, 0x8f, 0x2a, 0x5a, 0xe2, 0xf2, 0x51, 0x76, 0x95, 0x08, 0xf5, 0x60, 0x16, 0x38, 0xb4,
    0x8a, 0x5c, 0xd1, 0x15, 0x53, 0x1f, 0xf1, 0xbd, 0x9e, 0x76, 0x2e, 0x3d, 0xf4, 0x2e, 0xd0, 0x10,
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
