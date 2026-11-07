use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub(super) fn read_bounded(path: &Path, limit: usize) -> anyhow::Result<Vec<u8>> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "file is missing, linked, or exceeds its size limit"
    );
    ensure!(
        metadata.len() <= limit as u64,
        "file is missing, linked, or exceeds its size limit"
    );
    fs::read(path).with_context(|| format!("read {}", path.display()))
}

pub(super) fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "artifact is not a regular file"
    );
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .context("read artifact for SHA-256")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub(super) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(super) fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> anyhow::Result<()> {
    let mut file = File::create(path).context("create promotion output")?;
    serde_json::to_writer_pretty(&mut file, value).context("serialize promotion output")?;
    file.write_all(b"\n").context("finish promotion output")?;
    file.sync_all().context("sync promotion output")
}

pub(super) fn release_tag(generation: u64, digest: &str) -> anyhow::Result<String> {
    let hex = digest
        .strip_prefix("sha256:")
        .context("image digest must use sha256")?;
    ensure!(hex.len() >= 12, "image digest is too short");
    Ok(format!(
        "computer-use-images-v1-{generation}-{}",
        &hex[..12]
    ))
}

pub(super) fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn unix_time_ms() -> anyhow::Result<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?;
    u64::try_from(duration.as_millis()).context("Unix time overflow")
}
