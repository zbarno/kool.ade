use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

pub(super) fn fingerprint(worktree: &Path, relative: &str) -> anyhow::Result<Option<String>> {
    use std::os::unix::fs::PermissionsExt;
    let relative_path = Path::new(relative);
    anyhow::ensure!(
        relative_path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Invalid verification artifact path"
    );
    let path = worktree.join(relative_path);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return Ok(None);
    };
    let canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    if !canonical.starts_with(worktree.canonicalize()?) {
        return Ok(None);
    }
    // Files below a symlink directory never acquire verification provenance.
    let mut parent = path.parent();
    while let Some(directory) = parent.filter(|directory| *directory != worktree) {
        if fs::symlink_metadata(directory)?.file_type().is_symlink() {
            return Ok(None);
        }
        parent = directory.parent();
    }
    if metadata.file_type().is_symlink() {
        // npm creates .bin and workspace links. Preserve the link itself, never
        // follow it to hash content or admit links outside the worktree.
        let target = fs::read_link(&path)?;
        if !target.is_relative() {
            return Ok(None);
        }
        let target = target
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 verification link"))?;
        return Ok(Some(format!(
            "link:{:o}:{:x}",
            metadata.permissions().mode(),
            Sha256::digest(target.as_bytes())
        )));
    }
    if !metadata.is_file() {
        return Ok(None);
    }
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(Some(format!(
        "{:o}:{:x}",
        metadata.permissions().mode(),
        digest.finalize()
    )))
}
