use std::{fs, io::Read, path::Path};

use crate::execution::{
    errors::ExecutionError,
    remote::{WorkspaceEntryKindV1, staging::VerifiedWorkspaceResult},
};

use super::incompatible;

pub(super) fn result_to_archive(
    result: &VerifiedWorkspaceResult,
) -> Result<Vec<u8>, ExecutionError> {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in &result.manifest().entries {
        let relative = Path::new(&entry.path);
        let source = checked_source(result.materialized_path(), relative)?;
        let mut header = tar::Header::new_gnu();
        header.set_path(relative).map_err(|_| incompatible())?;
        match entry.kind {
            WorkspaceEntryKindV1::Directory => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_cksum();
                builder
                    .append(&header, std::io::empty())
                    .map_err(|_| incompatible())?;
            }
            WorkspaceEntryKindV1::File => {
                let metadata = fs::symlink_metadata(&source).map_err(|_| incompatible())?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() != entry.size_bytes
                {
                    return Err(incompatible());
                }
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(entry.size_bytes);
                header.set_mode(if entry.executable { 0o755 } else { 0o644 });
                header.set_cksum();
                let mut file = fs::File::open(source).map_err(|_| incompatible())?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).map_err(|_| incompatible())?;
                use sha2::Digest;
                let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
                if bytes.len() as u64 != entry.size_bytes
                    || entry.sha256.as_deref() != Some(digest.as_str())
                {
                    return Err(incompatible());
                }
                builder
                    .append(&header, bytes.as_slice())
                    .map_err(|_| incompatible())?;
            }
            WorkspaceEntryKindV1::Symlink => return Err(incompatible()),
        }
    }
    builder.finish().map_err(|_| incompatible())?;
    builder.into_inner().map_err(|_| incompatible())
}

fn checked_source(root: &Path, relative: &Path) -> Result<std::path::PathBuf, ExecutionError> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(incompatible());
    }
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, part) in components.iter().enumerate() {
        current.push(part.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|_| incompatible())?;
        if metadata.file_type().is_symlink() || (index + 1 < components.len() && !metadata.is_dir())
        {
            return Err(incompatible());
        }
    }
    Ok(current)
}
