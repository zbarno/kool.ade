use std::{env, io, io::Read, path::PathBuf};

use anyhow::{Context, bail};
use ed25519_dalek::SigningKey;

use super::catalog::{
    CandidateArchitecture, SigningRequest, sign_candidate, verify_catalog_bundle,
    verify_release_assets,
};

pub(super) fn run() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("sign") => {
            let [
                candidate,
                companion,
                previous_release,
                output,
                previous_catalog,
                previous_lock,
                source_revision,
                generation,
                attestations,
                architecture,
            ] = collect_args(args)?;
            let candidate = PathBuf::from(candidate);
            let companion = PathBuf::from(companion);
            let previous_release = PathBuf::from(previous_release);
            let output = PathBuf::from(output);
            let previous_catalog = PathBuf::from(previous_catalog);
            let previous_lock = PathBuf::from(previous_lock);
            let attestations = PathBuf::from(attestations);
            let mut secret = zeroize::Zeroizing::new(String::new());
            io::stdin()
                .read_to_string(&mut secret)
                .context("read signing seed from stdin")?;
            let signing_key = parse_signing_seed(&secret)?;
            sign_candidate(
                SigningRequest {
                    candidate_dir: &candidate,
                    companion_binary: &companion,
                    previous_release_dir: &previous_release,
                    output_dir: &output,
                    previous_catalog_path: &previous_catalog,
                    previous_lock_path: &previous_lock,
                    expected_source_revision: &source_revision,
                    generation: generation
                        .parse::<u64>()
                        .context("catalog generation must be an integer")?,
                    attestations_path: &attestations,
                    architecture: architecture.parse::<CandidateArchitecture>()?,
                },
                &signing_key,
            )?;
        }
        Some("verify") => {
            let [catalog, lock] = collect_args(args)?;
            let catalog = PathBuf::from(catalog);
            let lock = PathBuf::from(lock);
            verify_catalog_bundle(&catalog, &lock)?;
        }
        Some("verify-release-assets") => {
            let [catalog, lock, assets] = collect_args(args)?;
            let catalog = PathBuf::from(catalog);
            let lock = PathBuf::from(lock);
            let assets = PathBuf::from(assets);
            verify_release_assets(&catalog, &lock, &assets)?;
        }
        _ => bail!(
            "usage: computer-use-image-promotion sign CANDIDATE_DIR COMPANION_BINARY PREVIOUS_RELEASE_DIR OUTPUT_DIR PREVIOUS_CATALOG PREVIOUS_LOCK SOURCE_SHA GENERATION ATTESTATIONS_JSON ARCHITECTURE (seed hex on stdin) | verify CATALOG LOCK | verify-release-assets CATALOG LOCK ASSETS_DIR"
        ),
    }
    Ok(())
}

fn collect_args<const N: usize>(args: impl Iterator<Item = String>) -> anyhow::Result<[String; N]> {
    let collected: Vec<_> = args.collect();
    collected
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected {N} arguments"))
}

fn parse_signing_seed(secret: &str) -> anyhow::Result<SigningKey> {
    let encoded = secret.trim();
    let encoded = encoded.strip_prefix("ed25519-seed-hex:").unwrap_or(encoded);
    if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("signing seed must be exactly 32 bytes encoded as 64 hexadecimal characters");
    }
    let mut seed = zeroize::Zeroizing::new([0_u8; 32]);
    for (index, pair) in encoded.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        seed[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Ok(SigningKey::from_bytes(&seed))
}

fn hex_digit(byte: u8) -> anyhow::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => anyhow::bail!("invalid hexadecimal signing seed"),
    }
}

#[cfg(test)]
#[path = "command/tests.rs"]
mod tests;
