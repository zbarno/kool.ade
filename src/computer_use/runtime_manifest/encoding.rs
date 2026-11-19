use sha2::{Digest, Sha256};

use super::manifest::{RuntimeManifestErrorV1, RuntimeManifestV1};

/// Domain separation for release signing. A signature minted under the guest
/// image catalog domain (`koolade.computer-image-catalog.v1`) or any other
/// Kool.a.de domain must never verify as a runtime manifest, and vice versa.
const SIGNING_DOMAIN: &[u8] = b"koolade.computer-runtime-manifest.v1\0";

/// Bytes an offline release signer must sign for this manifest.
pub(super) fn signing_payload_bytes(
    manifest: &RuntimeManifestV1,
) -> Result<Vec<u8>, RuntimeManifestErrorV1> {
    let unsigned = UnsignedManifestV1 {
        schema_version: manifest.schema_version,
        release_version: &manifest.release_version,
        generation: manifest.generation,
        issued_at_unix_ms: manifest.issued_at_unix_ms,
        expires_at_unix_ms: manifest.expires_at_unix_ms,
        signer_key_id: &manifest.signer_key_id,
        artifacts: &manifest.artifacts,
    };
    let value =
        serde_json::to_value(unsigned).map_err(|_| RuntimeManifestErrorV1::EncodingFailed)?;
    let mut bytes = SIGNING_DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(&value).map_err(|_| RuntimeManifestErrorV1::EncodingFailed)?);
    Ok(bytes)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsignedManifestV1<'a> {
    schema_version: u16,
    release_version: &'a str,
    generation: u64,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    signer_key_id: &'a str,
    artifacts: &'a [super::manifest::RuntimeArtifactV1],
}

/// Canonical bytes this manifest hashes to (includes the signature field, so
/// the digest binds the envelope as distributed).
pub(super) fn canonical_manifest_bytes(
    manifest: &RuntimeManifestV1,
) -> Result<Vec<u8>, RuntimeManifestErrorV1> {
    let value =
        serde_json::to_value(manifest).map_err(|_| RuntimeManifestErrorV1::EncodingFailed)?;
    serde_json::to_vec(&value).map_err(|_| RuntimeManifestErrorV1::EncodingFailed)
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

pub(super) fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

pub(super) fn is_release_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && !value.contains("..")
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
}

pub(super) fn is_signature_hex(value: &str) -> bool {
    value.len() == 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn decode_lower_hex(value: &str) -> Result<Vec<u8>, ()> {
    if !value.len().is_multiple_of(2) {
        return Err(());
    }
    let (pairs, rest) = value.as_bytes().as_chunks::<2>();
    debug_assert!(rest.is_empty());
    let mut decoded = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let hi = (pair[0] as char).to_digit(16).ok_or(())? as u8;
        let lo = (pair[1] as char).to_digit(16).ok_or(())? as u8;
        decoded.push(hi << 4 | lo);
    }
    Ok(decoded)
}
