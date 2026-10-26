use std::{fs::File, io::Read, path::Path};

use pgp::{
    composed::{Deserializable, DetachedSignature, SignedPublicKey},
    types::KeyDetails,
};
use sha2::{Digest, Sha256};

use crate::app::bootstrap_remediation::claude_code_install::recipe;

pub(super) fn verify_signed_manifest(
    manifest: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !manifest.is_empty() && manifest.len() <= 256 * 1024,
        "Anthropic manifest exceeds its size bound"
    );
    anyhow::ensure!(
        !signature.is_empty() && signature.len() <= 16 * 1024,
        "Anthropic manifest signature exceeds its size bound"
    );
    anyhow::ensure!(
        !public_key.is_empty() && public_key.len() <= 64 * 1024,
        "Anthropic public key exceeds its size bound"
    );
    let key_text = std::str::from_utf8(public_key)?;
    let (mut keys, _) = SignedPublicKey::from_string_many(key_text)?;
    let key = keys
        .next()
        .ok_or_else(|| anyhow::anyhow!("Anthropic public key is missing"))??;
    anyhow::ensure!(keys.next().is_none(), "unexpected additional OpenPGP keys");
    key.verify_bindings()?;
    let fingerprint = format!("{:X}", key.primary_key.fingerprint());
    anyhow::ensure!(
        fingerprint == recipe::KEY_FINGERPRINT,
        "Anthropic manifest key fingerprint does not match the pinned key"
    );
    let signature_text = std::str::from_utf8(signature)?;
    let (mut signatures, _) = DetachedSignature::from_string_many(signature_text)?;
    let signature = signatures
        .next()
        .ok_or_else(|| anyhow::anyhow!("Anthropic manifest signature is missing"))??;
    anyhow::ensure!(
        signatures.next().is_none(),
        "unexpected additional manifest signatures"
    );
    let primary_verified = signature.verify(&key.primary_key, manifest).is_ok();
    let subkey_verified = !primary_verified
        && key
            .public_subkeys
            .iter()
            .any(|subkey| signature.verify(&subkey.key, manifest).is_ok());
    anyhow::ensure!(
        primary_verified || subkey_verified,
        "Anthropic manifest signature is invalid"
    );
    validate_manifest_contents(manifest)
}

fn validate_manifest_contents(manifest: &[u8]) -> anyhow::Result<()> {
    let document: serde_json::Value = serde_json::from_slice(manifest)?;
    anyhow::ensure!(
        document.get("version").and_then(serde_json::Value::as_str) == Some(recipe::RELEASE),
        "signed Anthropic manifest version differs from the pinned release"
    );
    let platform = document
        .get("platforms")
        .and_then(|value| value.get("linux-x64"))
        .ok_or_else(|| anyhow::anyhow!("signed Anthropic manifest lacks linux-x64"))?;
    anyhow::ensure!(
        platform.get("binary").and_then(serde_json::Value::as_str) == Some("claude")
            && platform.get("checksum").and_then(serde_json::Value::as_str)
                == Some(recipe::BINARY_SHA256)
            && platform.get("size").and_then(serde_json::Value::as_u64)
                == Some(recipe::BINARY_SIZE),
        "signed Anthropic manifest linux-x64 artifact differs from the pinned recipe"
    );
    Ok(())
}

pub(super) fn verify_binary(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() == recipe::BINARY_SIZE,
        "downloaded Claude Code binary size does not match the signed manifest"
    );
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| anyhow::anyhow!("Claude binary size overflow"))?;
        anyhow::ensure!(
            total <= recipe::BINARY_SIZE,
            "Claude binary exceeds its size bound"
        );
        hasher.update(&buffer[..read]);
    }
    anyhow::ensure!(
        total == recipe::BINARY_SIZE && format!("{:x}", hasher.finalize()) == recipe::BINARY_SHA256,
        "downloaded Claude Code binary does not match the signed manifest checksum"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned_manifest() -> serde_json::Value {
        serde_json::json!({
            "version": recipe::RELEASE,
            "platforms": {
                "linux-x64": {
                    "binary": "claude",
                    "checksum": recipe::BINARY_SHA256,
                    "size": recipe::BINARY_SIZE
                }
            }
        })
    }

    #[test]
    fn manifest_selection_rejects_version_platform_checksum_and_size_drift() {
        let valid = pinned_manifest();
        validate_manifest_contents(&serde_json::to_vec(&valid).unwrap()).unwrap();
        for mutate in [0, 1, 2, 3] {
            let mut changed = valid.clone();
            match mutate {
                0 => changed["version"] = "2.1.311".into(),
                1 => changed["platforms"]["linux-x64"]["binary"] = "claude.exe".into(),
                2 => changed["platforms"]["linux-x64"]["checksum"] = "0".repeat(64).into(),
                _ => changed["platforms"]["linux-x64"]["size"] = 1_u64.into(),
            }
            assert!(validate_manifest_contents(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
    }

    #[test]
    fn invalid_signature_and_key_input_fails_closed() {
        let manifest = serde_json::to_vec(&pinned_manifest()).unwrap();
        assert!(verify_signed_manifest(&manifest, b"not a signature", b"not a key").is_err());
        assert!(verify_signed_manifest(&manifest, b"", b"").is_err());
    }
}
