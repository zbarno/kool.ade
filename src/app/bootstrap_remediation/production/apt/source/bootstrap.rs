#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::app::bootstrap_remediation::{
    VerifiedArtifact,
    trusted_assets::{
        DOCKER_APT_KEY, DOCKER_APT_KEY_SHA256, DOCKER_APT_SOURCES, DOCKER_APT_SOURCES_SHA256,
    },
};
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

use super::{
    DOCKER_KEYRING, LEGACY_SOURCES, SOURCES_DIR, keyring::verify_docker_signing_key,
    parser::inspect_source_contents, read_bounded_file, trusted_root_directory_metadata,
    verify_root_directory, verify_root_file_metadata,
};

pub(crate) const DOCKER_SOURCES_PATH: &str = "/etc/apt/sources.list.d/koolade-docker.sources";
const DOCKER_DEBIAN_SOURCES: &[u8] = b"Types: deb\nURIs: https://download.docker.com/linux/debian\nSuites: trixie\nComponents: stable\nArchitectures: amd64\nSigned-By: /etc/apt/keyrings/docker.asc\n";
const DOCKER_DEBIAN_SOURCES_SHA256: &str =
    "61246a32aa079ddf3d8a57d922a7fc311d82f23379bfe0bec65f7c685c141e97";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DockerSourceBootstrap {
    pub(crate) create_keyrings_directory: bool,
    pub(crate) install_key: bool,
    pub(crate) install_sources: bool,
}

pub(crate) fn docker_source_bootstrap(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<DockerSourceBootstrap> {
    #[cfg(target_os = "linux")]
    {
        verify_root_directory(Path::new("/etc"))?;
        verify_root_directory(Path::new("/etc/apt"))?;
        verify_root_directory(Path::new(SOURCES_DIR))?;
        let create_keyrings_directory = verify_optional_keyring_directory()?;

        let install_key = match std::fs::symlink_metadata(DOCKER_KEYRING) {
            Ok(_) => {
                let (contents, metadata) = read_bounded_file(Path::new(DOCKER_KEYRING))?;
                verify_root_file_metadata(&metadata)?;
                verify_docker_signing_key(&contents)?;
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => return Err(error.into()),
        };

        let mut docker_entries = 0_usize;
        let mut invalid_entry = false;
        let mut unapproved_source = false;
        let legacy = Path::new(LEGACY_SOURCES);
        if legacy.exists() {
            inspect_source_file(
                recipe,
                legacy,
                false,
                &mut docker_entries,
                &mut invalid_entry,
                &mut unapproved_source,
            )?;
        }
        for entry in std::fs::read_dir(SOURCES_DIR)? {
            let path = entry?.path();
            let extension = path.extension().and_then(|value| value.to_str());
            if matches!(extension, Some("list" | "sources")) {
                inspect_source_file(
                    recipe,
                    &path,
                    extension == Some("sources"),
                    &mut docker_entries,
                    &mut invalid_entry,
                    &mut unapproved_source,
                )?;
            }
        }
        anyhow::ensure!(
            !invalid_entry && !unapproved_source && docker_entries <= 1,
            "existing APT source configuration conflicts with the approved recipe"
        );
        if docker_entries == 1 {
            return Ok(DockerSourceBootstrap {
                create_keyrings_directory,
                install_key,
                install_sources: false,
            });
        }

        match std::fs::symlink_metadata(DOCKER_SOURCES_PATH) {
            Ok(_) => {
                let (contents, metadata) = read_bounded_file(Path::new(DOCKER_SOURCES_PATH))?;
                verify_root_file_metadata(&metadata)?;
                anyhow::ensure!(
                    contents == source_contents(recipe)?,
                    "Kool.ad/e Docker source path already contains different administrator data"
                );
                anyhow::bail!("Docker source file is present but was not recognized as active")
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(DockerSourceBootstrap {
            create_keyrings_directory,
            install_key,
            install_sources: true,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = recipe;
        anyhow::bail!("Docker APT repository setup is Linux-only")
    }
}

pub(crate) fn docker_source_file_artifacts(
    recipe: LinuxRootlessRecipeV1,
    setup: DockerSourceBootstrap,
) -> anyhow::Result<(Option<VerifiedArtifact>, Option<VerifiedArtifact>)> {
    let key = setup
        .install_key
        .then(|| {
            VerifiedArtifact::from_bundled_bytes(
                "bundled://docker-ce-archive-key",
                DOCKER_APT_KEY,
                DOCKER_APT_KEY_SHA256,
            )
        })
        .transpose()?;
    let (source_bytes, source_digest) = source_asset(recipe)?;
    let source_uri = match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64 => "bundled://docker-debian-apt-sources",
        _ => "bundled://docker-ubuntu-apt-sources",
    };
    let sources = setup
        .install_sources
        .then(|| VerifiedArtifact::from_bundled_bytes(source_uri, source_bytes, source_digest))
        .transpose()?;
    Ok((key, sources))
}

#[cfg(target_os = "linux")]
fn inspect_source_file(
    recipe: LinuxRootlessRecipeV1,
    path: &Path,
    deb822: bool,
    entries: &mut usize,
    invalid: &mut bool,
    unapproved: &mut bool,
) -> anyhow::Result<()> {
    let (contents, metadata) = read_bounded_file(path)?;
    verify_root_file_metadata(&metadata)?;
    let inspection = inspect_source_contents(&contents, deb822, recipe)?;
    *invalid |= inspection.has_invalid_docker_entry;
    *unapproved |= inspection.has_unapproved_source;
    *entries += inspection.exact_entries;
    Ok(())
}

fn source_asset(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<(&'static [u8], &'static str)> {
    match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64 => {
            Ok((DOCKER_DEBIAN_SOURCES, DOCKER_DEBIAN_SOURCES_SHA256))
        }
        LinuxRootlessRecipeV1::Ubuntu2404Amd64 | LinuxRootlessRecipeV1::PopOs2404Amd64 => {
            Ok((DOCKER_APT_SOURCES, DOCKER_APT_SOURCES_SHA256))
        }
        LinuxRootlessRecipeV1::Fedora45Amd64 => anyhow::bail!("Fedora uses DNF"),
    }
}

fn source_contents(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<&'static [u8]> {
    Ok(source_asset(recipe)?.0)
}

#[cfg(target_os = "linux")]
fn verify_optional_keyring_directory() -> anyhow::Result<bool> {
    match std::fs::symlink_metadata("/etc/apt/keyrings") {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && trusted_root_directory_metadata(metadata.uid(), metadata.mode()),
                "APT keyring directory is not trusted"
            );
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
#[path = "bootstrap_tests.rs"]
mod tests;
