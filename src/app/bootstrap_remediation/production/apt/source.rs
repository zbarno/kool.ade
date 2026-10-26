mod bootstrap;
mod keyring;
mod parser;

pub(crate) use bootstrap::{docker_source_bootstrap, docker_source_file_artifacts};

use std::path::{Path, PathBuf};

use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;
use parser::inspect_source_contents;

const SOURCES_DIR: &str = "/etc/apt/sources.list.d";
const LEGACY_SOURCES: &str = "/etc/apt/sources.list";
const DOCKER_KEYRING: &str = "/etc/apt/keyrings/docker.asc";
const MAX_SOURCE_FILE_BYTES: u64 = 1024 * 1024;

#[cfg(test)]
pub(super) use parser::SourceInspection;

pub(super) fn trusted_root_owner_mode(owner_uid: u32, mode: u32) -> bool {
    owner_uid == 0 && mode & 0o022 == 0
}

pub(super) fn trusted_root_directory_metadata(owner_uid: u32, mode: u32) -> bool {
    trusted_root_owner_mode(owner_uid, mode) && mode & 0o700 == 0o700 && mode & 0o005 == 0o005
}

pub(super) fn trusted_root_file_metadata(owner_uid: u32, mode: u32) -> bool {
    trusted_root_owner_mode(owner_uid, mode) && mode & 0o404 == 0o404
}

pub(super) fn check_docker_source_configuration(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        verify_root_directory(Path::new("/etc"))?;
        verify_root_directory(Path::new("/etc/apt"))?;
        let source_dir = Path::new(SOURCES_DIR);
        verify_root_directory(source_dir)?;
        let mut paths = Vec::<PathBuf>::new();
        if let Some(path) = optional_source_path(Path::new(LEGACY_SOURCES))? {
            paths.push(path);
        }
        for entry in std::fs::read_dir(source_dir)? {
            let path = entry?.path();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if file_name.ends_with(".list") || file_name.ends_with(".sources") {
                paths.push(path);
            }
        }
        paths.sort();

        let mut exact_entries = 0;
        for path in paths {
            let (contents, metadata) = read_bounded_file(&path)?;
            let deb822 = path
                .extension()
                .is_some_and(|extension| extension == "sources");
            verify_root_file_metadata(&metadata)?;
            let inspection = inspect_source_contents(&contents, deb822, recipe)?;
            anyhow::ensure!(
                !inspection.has_unapproved_source && !inspection.has_invalid_docker_entry,
                "APT source configuration contains an unapproved active repository"
            );
            exact_entries += inspection.exact_entries;
        }
        anyhow::ensure!(
            exact_entries == 1,
            "exactly one active Docker source for the approved stable amd64 recipe is required"
        );
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = recipe;
        anyhow::bail!("Linux rootless Docker package sources are Linux-only")
    }
}

pub(super) fn check_docker_keyring() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        for directory in ["/etc", "/etc/apt", "/etc/apt/keyrings"] {
            verify_root_directory(Path::new(directory))?;
        }
        let (contents, metadata) = read_bounded_file(Path::new(DOCKER_KEYRING))?;
        verify_root_file_metadata(&metadata)?;
        keyring::verify_docker_signing_key(&contents)
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("Linux rootless Docker package keyrings are Linux-only")
    }
}

#[cfg(target_os = "linux")]
fn verify_root_directory(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && trusted_root_directory_metadata(metadata.uid(), metadata.mode()),
        "APT configuration directory is not a trusted root-owned directory"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn verify_root_file_metadata(metadata: &std::fs::Metadata) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    anyhow::ensure!(
        metadata.is_file() && trusted_root_file_metadata(metadata.uid(), metadata.mode()),
        "APT source or Docker keyring is not a trusted root-owned file"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn optional_source_path(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(Some(path.to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(target_os = "linux")]
pub(super) fn read_bounded_file(path: &Path) -> anyhow::Result<(Vec<u8>, std::fs::Metadata)> {
    use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt};

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= MAX_SOURCE_FILE_BYTES,
        "APT source or keyring is not a bounded regular file"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_SOURCE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_SOURCE_FILE_BYTES,
        "APT source or keyring exceeds the file size limit"
    );
    Ok((bytes, metadata))
}

#[cfg(not(target_os = "linux"))]
fn verify_root_directory(_path: &Path) -> anyhow::Result<()> {
    anyhow::bail!("APT configuration is Linux-only")
}

#[cfg(not(target_os = "linux"))]
fn verify_root_file_metadata(_metadata: &std::fs::Metadata) -> anyhow::Result<()> {
    anyhow::bail!("APT configuration is Linux-only")
}

#[cfg(not(target_os = "linux"))]
fn optional_source_path(_path: &Path) -> anyhow::Result<Option<PathBuf>> {
    anyhow::bail!("APT configuration is Linux-only")
}

#[cfg(not(target_os = "linux"))]
fn read_bounded_file(_path: &Path) -> anyhow::Result<(Vec<u8>, std::fs::Metadata)> {
    anyhow::bail!("APT configuration is Linux-only")
}

#[cfg(test)]
pub(super) fn inspect_for_test(contents: &[u8], deb822: bool) -> anyhow::Result<SourceInspection> {
    inspect_source_contents(contents, deb822, LinuxRootlessRecipeV1::Ubuntu2404Amd64)
}

#[cfg(test)]
pub(super) fn inspect_for_recipe_test(
    contents: &[u8],
    deb822: bool,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<SourceInspection> {
    inspect_source_contents(contents, deb822, recipe)
}
