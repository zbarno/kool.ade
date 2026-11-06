use sha2::{Digest, Sha256};

use super::{GuestImageEntryV1, GuestImageManifestV1};
use crate::computer_use::guest_image::ImageCacheErrorV1;

const SIGNING_DOMAIN: &[u8] = b"koolade.computer-image-catalog.v1\0";

pub(super) fn signing_payload_bytes(
    manifest: &GuestImageManifestV1,
) -> Result<Vec<u8>, ImageCacheErrorV1> {
    let unsigned = UnsignedManifestV1 {
        schema_version: manifest.schema_version,
        catalog_version: &manifest.catalog_version,
        catalog_generation: manifest.catalog_generation,
        issued_at_unix_ms: manifest.issued_at_unix_ms,
        expires_at_unix_ms: manifest.expires_at_unix_ms,
        signer_key_id: &manifest.signer_key_id,
        images: &manifest.images,
    };
    let value = serde_json::to_value(unsigned)
        .map_err(|_| ImageCacheErrorV1::InvalidManifest("cannot encode manifest"))?;
    let mut bytes = SIGNING_DOMAIN.to_vec();
    bytes.extend(
        serde_json::to_vec(&value)
            .map_err(|_| ImageCacheErrorV1::InvalidManifest("cannot encode manifest"))?,
    );
    Ok(bytes)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsignedManifestV1<'a> {
    schema_version: u16,
    catalog_version: &'a str,
    catalog_generation: u64,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    signer_key_id: &'a str,
    images: &'a [GuestImageEntryV1],
}

pub(super) fn canonical_manifest_bytes(
    manifest: &GuestImageManifestV1,
) -> Result<Vec<u8>, ImageCacheErrorV1> {
    let value = serde_json::to_value(manifest)
        .map_err(|_| ImageCacheErrorV1::InvalidManifest("cannot encode manifest"))?;
    serde_json::to_vec(&value)
        .map_err(|_| ImageCacheErrorV1::InvalidManifest("cannot encode manifest"))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub(super) fn is_git_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

pub(super) fn is_release_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && !value.contains("..")
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && (bytes[bytes.len() - 1].is_ascii_lowercase() || bytes[bytes.len() - 1].is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-._".contains(byte))
}

pub(super) fn decode_hex(value: &str) -> Result<Vec<u8>, ImageCacheErrorV1> {
    let bytes = value.as_bytes();
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(ImageCacheErrorV1::InvalidManifest("invalid signature"));
    }
    pairs
        .iter()
        .map(|pair| {
            let digit = |value: u8| match value {
                b'0'..=b'9' => Ok(value - b'0'),
                b'a'..=b'f' => Ok(value - b'a' + 10),
                _ => Err(ImageCacheErrorV1::InvalidManifest("invalid signature")),
            };
            Ok((digit(pair[0])? << 4) | digit(pair[1])?)
        })
        .collect()
}
