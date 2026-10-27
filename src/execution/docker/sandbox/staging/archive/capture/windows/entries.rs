use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use tar::{Builder, EntryType};

use super::super::super::{Fingerprint, Manifest, hash, validate_relative};
use super::{
    enumeration,
    format::{append_header, mode_for, path_key},
    index::Snapshot as GitIndex,
    metadata::{
        ATTRIBUTE_DEVICE, ATTRIBUTE_DIRECTORY, ATTRIBUTE_REPARSE_POINT, Metadata, ensure_directory,
        ensure_not_reparse, ensure_stable_directory, ensure_stable_file, handle_identity, metadata,
    },
    native::{self, OPEN_REPARSE_ATTRIBUTES},
};

pub(super) use super::metadata::verify_directory;

struct Child {
    name: String,
    file_id: u64,
    attributes: u32,
}

pub(super) fn append_directory(
    directory: &File,
    relative: &Path,
    builder: &mut Builder<Vec<u8>>,
    manifest: &mut Manifest,
    total_bytes: &mut u64,
    byte_limit: u64,
    entry_limit: usize,
    read_only: bool,
    excluded_paths: &[String],
    git_index: &GitIndex,
    scope: Option<&Path>,
    captured_paths: &mut Vec<String>,
) -> anyhow::Result<()> {
    let before = metadata(directory)?;
    ensure_directory(&before)?;
    let before_id = handle_identity(directory)?;
    let children = enumeration::read_children(directory)?;
    let child_paths = children
        .iter()
        .filter(|child| !child.name.eq_ignore_ascii_case(".git"))
        .map(|child| relative.join(&child.name))
        .collect::<Vec<_>>();
    super::super::super::windows_paths::validate_collisions(
        child_paths.iter().map(PathBuf::as_path),
    )?;
    for child in children {
        if child.name.eq_ignore_ascii_case(".git") {
            continue;
        }
        let child_relative = relative.join(&child.name);
        validate_relative(&child_relative)?;
        captured_paths.push(child_relative.to_string_lossy().replace('\\', "/"));
        let child_key = path_key(&child_relative.to_string_lossy());
        if excluded_paths
            .iter()
            .any(|path| path_key(path) == child_key)
        {
            continue;
        }
        ensure!(
            manifest.len() < entry_limit,
            "workspace has too many entries"
        );
        ensure!(
            child.attributes & (ATTRIBUTE_REPARSE_POINT | ATTRIBUTE_DEVICE) == 0,
            "workspace contains a reparse point or special file"
        );
        let file = native::open_at(directory, &child.name, OPEN_REPARSE_ATTRIBUTES)?;
        let opened = metadata(&file)?;
        ensure_not_reparse(&opened)?;
        let identity = handle_identity(&file)?;
        ensure!(
            identity.1 != 0 && identity.1 == child.file_id,
            "workspace entry changed while enumerating"
        );
        if opened.standard.Directory {
            append_header(
                builder,
                &child_relative,
                EntryType::Directory,
                0,
                mode_for(true, false, read_only),
                &[],
            )?;
            ensure!(
                manifest
                    .insert(child_relative.clone(), Fingerprint::Directory)
                    .is_none(),
                "workspace path collided"
            );
            append_directory(
                &file,
                &child_relative,
                builder,
                manifest,
                total_bytes,
                byte_limit,
                entry_limit,
                read_only,
                excluded_paths,
                git_index,
                scope,
                captured_paths,
            )?;
        } else {
            append_file(
                file,
                opened,
                identity,
                &child_relative,
                builder,
                manifest,
                total_bytes,
                byte_limit,
                entry_limit,
                read_only,
                git_index.executable_for(&child_relative, scope),
            )?;
        }
    }
    ensure_stable_directory(
        &before,
        &metadata(directory)?,
        before_id,
        handle_identity(directory)?,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_file(
    mut file: File,
    before: Metadata,
    before_id: (u32, u64),
    relative: &Path,
    builder: &mut Builder<Vec<u8>>,
    manifest: &mut Manifest,
    total_bytes: &mut u64,
    byte_limit: u64,
    entry_limit: usize,
    read_only: bool,
    executable: bool,
) -> anyhow::Result<()> {
    ensure!(
        !before.standard.Directory
            && before.attributes.FileAttributes & ATTRIBUTE_DIRECTORY == 0
            && !before.standard.DeletePending
            && before.standard.NumberOfLinks == 1
            && before.standard.EndOfFile >= 0,
        "workspace contains a hard link or special file"
    );
    *total_bytes = total_bytes.saturating_add(before.standard.EndOfFile as u64);
    ensure!(
        *total_bytes <= byte_limit,
        "workspace exceeds staging budget"
    );
    let mut bytes = Vec::with_capacity((before.standard.EndOfFile as u64).min(1_048_576) as usize);
    (&mut file)
        .take(
            byte_limit
                .saturating_sub(*total_bytes)
                .saturating_add(before.standard.EndOfFile as u64)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .context("workspace file contents could not be read")?;
    let after = metadata(&file)?;
    ensure_stable_file(&before, &after, before_id, handle_identity(&file)?)?;
    ensure!(
        bytes.len() as u64 == before.standard.EndOfFile as u64,
        "workspace file changed while staging"
    );
    let digest = hash(&bytes);
    let mode = if executable { 0o755 } else { 0o644 };
    append_header(
        builder,
        relative,
        EntryType::Regular,
        bytes.len() as u64,
        mode_for(false, executable, read_only),
        &bytes,
    )?;
    ensure!(
        manifest.len() < entry_limit,
        "workspace has too many entries"
    );
    ensure!(
        manifest
            .insert(
                relative.to_path_buf(),
                Fingerprint::File {
                    sha256: digest,
                    size: bytes.len() as u64,
                    mode,
                },
            )
            .is_none(),
        "workspace path collided"
    );
    Ok(())
}
