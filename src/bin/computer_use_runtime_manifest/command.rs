use std::{env, fs, io, io::Read, path::Path};

use anyhow::{Context, bail};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use zeroize::{Zeroize, Zeroizing};

use koolade::computer_use::runtime_manifest::{
    BUNDLED_RUNTIME_MANIFEST_FILENAME, RUNTIME_MANIFEST_SCHEMA_VERSION, RuntimeManifestTrustV1,
    RuntimeManifestV1, verify_runtime_manifest,
};

pub(super) fn run() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("sign") => {
            let [
                release_version,
                generation,
                issued_at,
                expires_at,
                key_id,
                template,
                output,
            ] = collect_args(args)?;
            let mut seed = Zeroizing::new(String::new());
            io::stdin()
                .read_to_string(&mut seed)
                .context("read signing seed from stdin")?;
            let signing_key = parse_signing_seed(&seed)?;
            let template_text = fs::read_to_string(&template)
                .with_context(|| format!("read template {template}"))?;
            let mut manifest: RuntimeManifestV1 = serde_json::from_str(&template_text)
                .with_context(|| {
                    format!("template {template} must be a runtime manifest JSON document")
                })?;
            // Identity and timing travel on the command line so the signed
            // envelope records operator intent verbatim; the template supplies
            // only the artifact inventory (plus a placeholder signature).
            manifest.schema_version = RUNTIME_MANIFEST_SCHEMA_VERSION;
            manifest.release_version = release_version;
            manifest.generation = parse_positive_u64(&generation, "GENERATION")?;
            manifest.issued_at_unix_ms = parse_positive_u64(&issued_at, "ISSUED_MS")?;
            manifest.expires_at_unix_ms = parse_positive_u64(&expires_at, "EXPIRES_MS")?;
            manifest.signer_key_id = key_id;
            let signature = signing_key.sign(&manifest.signing_payload_bytes()?);
            manifest.signature_hex = hex_encode(&signature.to_bytes());
            let mut rendered = serde_json::to_vec_pretty(&manifest)?;
            rendered.push(b'\n');
            fs::write(&output, &rendered).with_context(|| format!("write {output}"))?;
            rendered.zeroize();
            println!("signed runtime manifest written to {output}");
            Ok(())
        }
        Some("verify") => {
            let [bundle_root, pin, now_ms] = collect_args(args)?;
            let pin = TrustPinDocument::from_file(Path::new(&pin))?;
            let trust = RuntimeManifestTrustV1::from_application_pinned_keys(
                pin.pinned_keys()?,
                pin.minimum_generation,
            )
            .map_err(|_| anyhow::anyhow!("pin file is not a valid trust pin"))?;
            let now_ms = parse_positive_u64(&now_ms, "NOW_MS")?;
            let bytes = fs::read(Path::new(&bundle_root).join(BUNDLED_RUNTIME_MANIFEST_FILENAME))
                .with_context(|| format!("read bundled manifest from {bundle_root}"))?;
            let verified = verify_runtime_manifest(&bytes, &trust, now_ms)?;
            println!("runtime manifest verified");
            println!("  digest:     {}", verified.digest());
            println!("  release:    {}", verified.release_version());
            println!("  generation: {}", verified.generation());
            println!("  signer:     {}", verified.signer_key_id());
            println!("  expires at: {}", verified.expires_at_unix_ms());
            for artifact in verified.artifacts() {
                println!(
                    "  artifact:   {:?} {} {} ({})",
                    artifact.platform(),
                    artifact.kind(),
                    artifact.bundle_relative_path(),
                    artifact.digest()
                );
            }
            Ok(())
        }
        Some("inspect") => {
            let [file] = collect_args(args)?;
            let text = fs::read_to_string(&file).with_context(|| format!("read {file}"))?;
            let value: serde_json::Value =
                serde_json::from_str(&text).with_context(|| format!("{file} is not JSON"))?;
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        _ => bail!(concat!(
            "usage: computer-use-runtime-manifest ",
            "sign RELEASE_VERSION GENERATION ISSUED_MS EXPIRES_MS KEY_ID TEMPLATE_JSON OUTPUT_JSON (seed hex on stdin) ",
            "| verify BUNDLE_ROOT_DIR PIN_JSON NOW_MS ",
            "| inspect FILE"
        )),
    }
}

/// Operator-facing trust pin. Public verification keys only; the seed never
/// touches this file. The same invariants as the in-app store (unique UUID
/// ids, nonzero floor, non-empty key ring) apply.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TrustPinDocument {
    minimum_generation: u64,
    keys: Vec<PinnedVerifierEntry>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PinnedVerifierEntry {
    key_id: String,
    public_key_hex: String,
}

impl TrustPinDocument {
    fn from_file(path: &Path) -> anyhow::Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read pin file {}", path.display()))?;
        serde_json::from_str(&text)
            .context(concat!(
                "pin file must be JSON: ",
                "{\"minimumGeneration\": N, \"keys\": [{\"keyId\": \"<uuid>\", \"publicKeyHex\": \"<64 hex>\"}]}"
            ))
    }

    fn pinned_keys(&self) -> anyhow::Result<Vec<(String, VerifyingKey)>> {
        self.keys
            .iter()
            .map(|entry| {
                let bytes = decode_public_key_hex(&entry.public_key_hex)?;
                VerifyingKey::from_bytes(&bytes)
                    .map(|key| (entry.key_id.clone(), key))
                    .map_err(|_| {
                        anyhow::anyhow!("{} is not a 32-byte Ed25519 public key", entry.key_id)
                    })
            })
            .collect()
    }
}

fn decode_public_key_hex(encoded: &str) -> anyhow::Result<[u8; 32]> {
    if encoded.len() != 64
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("public keys must be 32 bytes in lowercase hexadecimal");
    }
    let mut decoded = [0_u8; 32];
    let (pairs, rest) = encoded.as_bytes().as_chunks::<2>();
    debug_assert!(rest.is_empty());
    for (index, pair) in pairs.iter().enumerate() {
        decoded[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Ok(decoded)
}

fn parse_signing_seed(seed: &str) -> anyhow::Result<SigningKey> {
    let encoded = seed.trim();
    let encoded = encoded.strip_prefix("ed25519-seed-hex:").unwrap_or(encoded);
    if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("signing seed must be exactly 32 bytes encoded as 64 hexadecimal characters");
    }
    let mut bytes = Zeroizing::new([0_u8; 32]);
    let (pairs, rest) = encoded.as_bytes().as_chunks::<2>();
    debug_assert!(rest.is_empty());
    for (index, pair) in pairs.iter().enumerate() {
        bytes[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Ok(SigningKey::from_bytes(&bytes))
}

fn hex_digit(byte: u8) -> anyhow::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => bail!("invalid hexadecimal byte"),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}

fn parse_positive_u64(raw: &str, label: &str) -> anyhow::Result<u64> {
    let value = raw
        .parse::<u64>()
        .with_context(|| format!("{label} must be a non-negative integer"))?;
    if value == 0 {
        bail!("{label} must be positive");
    }
    Ok(value)
}

fn collect_args<const N: usize>(args: impl Iterator<Item = String>) -> anyhow::Result<[String; N]> {
    let collected: Vec<_> = args.collect();
    collected
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected {N} arguments"))
}

#[cfg(test)]
#[path = "command/tests.rs"]
mod tests;
