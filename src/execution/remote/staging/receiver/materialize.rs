use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::{ChunkRecord, storage};
use crate::execution::remote::{
    WorkspaceEntryKindV1,
    staging::{MAX_TRANSFER_CHUNK_BYTES, TransferManifestV1, TransferPayloadV1},
};

pub(super) fn assemble(
    transfer_directory: &Path,
    manifest: &TransferManifestV1,
    records: &BTreeMap<(String, u64), ChunkRecord>,
) -> anyhow::Result<PathBuf> {
    let materialized = transfer_directory.join("materialized");
    match fs::symlink_metadata(&materialized) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            fs::remove_dir_all(&materialized)?
        }
        Ok(_) => anyhow::bail!("materialized transfer destination is not trusted"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temporary =
        transfer_directory.join(format!("assemble-{}", uuid::Uuid::new_v4().hyphenated()));
    fs::create_dir(&temporary)?;
    private_dir(&temporary)?;

    let result = (|| -> anyhow::Result<PathBuf> {
        let mut directories = BTreeSet::new();
        let mut symlinks = Vec::new();
        let workspace = match &manifest.payload {
            TransferPayloadV1::Workspace(workspace)
            | TransferPayloadV1::WorkspaceResult(workspace) => Some(workspace),
            TransferPayloadV1::Results(_) => None,
        };
        if let Some(workspace) = workspace {
            for entry in &workspace.entries {
                match entry.kind {
                    WorkspaceEntryKindV1::Directory => {
                        directories.insert(entry.path.clone());
                    }
                    WorkspaceEntryKindV1::Symlink => symlinks.push((
                        entry.path.clone(),
                        entry.symlink_target.clone().unwrap_or_default(),
                    )),
                    WorkspaceEntryKindV1::File => {}
                }
            }
        }
        for file in manifest.files() {
            let path = validated_path(file.path)?;
            for parent in path
                .ancestors()
                .skip(1)
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                directories.insert(path_string(parent)?);
            }
        }
        for directory in directories.sorted_by_depth() {
            let path = safe_destination(&temporary, directory)?;
            create_directory_path(&temporary, &path)?;
        }

        for file in manifest.files() {
            let output = safe_destination(&temporary, file.path)?;
            ensure_parent_directories(&temporary, &output)?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let mut destination = options.open(&output)?;
            let mut digest = Sha256::new();
            let mut total = 0_u64;
            let mut offset = 0_u64;
            while offset < file.size_bytes {
                let record = records
                    .get(&(file.path.to_owned(), offset))
                    .ok_or_else(|| anyhow::anyhow!("transfer file is missing a chunk"))?;
                let chunk_path = storage::chunk_path(
                    transfer_directory,
                    &storage::path_digest(file.path),
                    offset,
                );
                let bytes = storage::read_bounded(&chunk_path, MAX_TRANSFER_CHUNK_BYTES as u64)?;
                anyhow::ensure!(
                    bytes.len() == record.size_bytes && digest_bytes(&bytes) == record.sha256,
                    "stored transfer chunk failed integrity check"
                );
                let end = offset
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| anyhow::anyhow!("transfer file size overflow"))?;
                anyhow::ensure!(
                    end <= file.size_bytes
                        && (bytes.len() == MAX_TRANSFER_CHUNK_BYTES || end == file.size_bytes),
                    "transfer chunks have a gap or truncation"
                );
                destination.write_all(&bytes)?;
                digest.update(&bytes);
                total = end;
                offset = end;
            }
            anyhow::ensure!(
                total == file.size_bytes && format!("{:x}", digest.finalize()) == file.sha256,
                "transferred file digest does not match manifest"
            );
            destination.sync_all()?;
            set_file_mode(&output, file.executable)?;
        }

        #[cfg(not(unix))]
        anyhow::ensure!(
            symlinks.is_empty(),
            "workspace symlink materialization is unsupported on this platform"
        );
        #[cfg(unix)]
        for (path, target) in symlinks {
            let output = safe_destination(&temporary, &path)?;
            ensure_parent_directories(&temporary, &output)?;
            anyhow::ensure!(
                valid_symlink_target(&path, &target),
                "workspace symlink target is invalid"
            );
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, output)?;
        }

        fs::rename(&temporary, &materialized)?;
        sync_directory(transfer_directory)?;
        Ok(materialized)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

trait DepthSort {
    fn sorted_by_depth(&self) -> Vec<&String>;
}
impl DepthSort for BTreeSet<String> {
    fn sorted_by_depth(&self) -> Vec<&String> {
        let mut paths = self.iter().collect::<Vec<_>>();
        paths.sort_by_key(|path| (path.matches('/').count(), path.as_str()));
        paths
    }
}

fn validated_path(value: &str) -> anyhow::Result<PathBuf> {
    super::super::manifest::validate_transfer_path(value)?;
    Ok(PathBuf::from(value))
}

fn safe_destination(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    Ok(root.join(validated_path(relative)?))
}

fn create_directory_path(root: &Path, path: &Path) -> anyhow::Result<()> {
    ensure_parent_directories(root, path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) => anyhow::ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "transfer directory collides with an unsafe path"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            private_dir(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn ensure_parent_directories(root: &Path, path: &Path) -> anyhow::Result<()> {
    let relative = path.strip_prefix(root)?;
    let mut current = root.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let Component::Normal(part) = component else {
            anyhow::bail!("transfer parent path is invalid");
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "transfer parent is not a real directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
                private_dir(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn valid_symlink_target(path: &str, target: &str) -> bool {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('\\')
        || target.contains(':')
        || target.chars().any(char::is_control)
    {
        return false;
    }
    let mut depth = path.split('/').count().saturating_sub(1);
    for component in target.split('/') {
        match component {
            "" | "." => return false,
            ".." if depth == 0 => return false,
            ".." => depth -= 1,
            component if valid_symlink_component(component) => depth += 1,
            _ => return false,
        }
    }
    true
}

fn valid_symlink_component(component: &str) -> bool {
    if component.is_empty()
        || component == "."
        || component.ends_with('.')
        || component.ends_with(' ')
        || component
            .chars()
            .any(|character| "*?\"<>|".contains(character))
    {
        return false;
    }
    let basename = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(basename.len() == 4
            && (basename.starts_with("COM") || basename.starts_with("LPT"))
            && matches!(basename.as_bytes()[3], b'1'..=b'9'))
}

fn path_string(path: &Path) -> anyhow::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("transfer path is not UTF-8"))
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn private_dir(_path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_file_mode(path: &Path, executable: bool) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }),
        )?;
    }
    let _ = (path, executable);
    Ok(())
}

fn sync_directory(_path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    fs::File::open(_path)?.sync_all()?;
    Ok(())
}
