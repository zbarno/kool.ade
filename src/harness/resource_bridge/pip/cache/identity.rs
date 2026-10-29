use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) fn identity_directory(
    cache_root: &Path,
    lockfile_identity: &str,
) -> anyhow::Result<PathBuf> {
    let digest = lockfile_digest(lockfile_identity)?;
    Ok(cache_root.join("sha256").join(digest))
}

pub(super) fn lockfile_digest(identity: &str) -> anyhow::Result<String> {
    let digest = identity
        .strip_prefix("sha256:")
        .ok_or_else(|| anyhow::anyhow!("Pip lockfile identity must be SHA-256"))?;
    anyhow::ensure!(
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Pip lockfile identity must be SHA-256"
    );
    Ok(digest.to_ascii_lowercase())
}

pub(super) fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
