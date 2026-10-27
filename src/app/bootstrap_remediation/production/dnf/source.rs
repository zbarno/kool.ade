#[cfg(target_os = "linux")]
use std::{os::unix::fs::MetadataExt, path::Path};

use crate::app::bootstrap_remediation::{
    VerifiedArtifact,
    trusted_assets::{
        DOCKER_DNF_KEY, DOCKER_DNF_KEY_SHA256, DOCKER_DNF_REPO, DOCKER_DNF_REPO_SHA256,
    },
};

use super::key::verify_docker_signing_key;
#[cfg(target_os = "linux")]
use super::key::verify_fedora_signing_key as verify_fedora_key_fingerprint;
#[cfg(target_os = "linux")]
mod configuration;
#[cfg(target_os = "linux")]
mod overrides;
#[cfg(target_os = "linux")]
mod repositories;

#[cfg(target_os = "linux")]
const KEYS_DIR: &str = "/etc/pki/rpm-gpg";
#[cfg(target_os = "linux")]
const FEDORA_KEY_DIR: &str = "/usr/share/pki/rpm-gpg";
#[cfg(target_os = "linux")]
const FEDORA_KEY_PATH: &str = "/usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-45-x86_64";
#[cfg(target_os = "linux")]
pub(crate) const DOCKER_KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-koolade-docker";
#[cfg(target_os = "linux")]
pub(crate) const DOCKER_REPO_PATH: &str = "/etc/yum.repos.d/koolade-docker-ce.repo";
#[cfg(target_os = "linux")]
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[cfg(all(test, target_os = "linux"))]
#[path = "source_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DockerRepositoryBootstrap {
    pub(crate) install_key: bool,
    pub(crate) install_repo: bool,
}

pub(crate) fn docker_repository_bootstrap() -> anyhow::Result<DockerRepositoryBootstrap> {
    #[cfg(target_os = "linux")]
    {
        for directory in [
            "/etc",
            "/etc/dnf",
            "/etc/yum.repos.d",
            "/etc/pki",
            KEYS_DIR,
            "/usr/share",
            "/usr/share/dnf5",
            "/usr/share/dnf5/repos.d",
            "/usr/share/pki",
            FEDORA_KEY_DIR,
            "/var",
            "/var/cache",
            "/var/cache/libdnf5",
        ] {
            verify_root_directory(Path::new(directory))?;
        }
        verify_fedora_signing_key()?;
        repositories::inspect_existing_repositories()?;
        configuration::inspect_configuration_dropins()?;
        overrides::reject_candidate_repository_overrides()?;
        let install_key = match read_optional_root_file(Path::new(DOCKER_KEY_PATH))? {
            Some(contents) => {
                anyhow::ensure!(
                    contents == DOCKER_DNF_KEY,
                    "Docker Fedora key path already contains different administrator data"
                );
                verify_docker_signing_key(&contents)?;
                false
            }
            None => true,
        };
        let install_repo = match read_optional_root_file(Path::new(DOCKER_REPO_PATH))? {
            Some(contents) => {
                anyhow::ensure!(
                    contents == DOCKER_DNF_REPO,
                    "Docker Fedora repository path already contains different administrator data"
                );
                false
            }
            None => true,
        };
        Ok(DockerRepositoryBootstrap {
            install_key,
            install_repo,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("Docker DNF repository setup is Linux-only")
    }
}

pub(crate) fn docker_repository_file_artifacts(
    setup: DockerRepositoryBootstrap,
) -> anyhow::Result<(Option<VerifiedArtifact>, Option<VerifiedArtifact>)> {
    if setup.install_key {
        verify_docker_signing_key(DOCKER_DNF_KEY)?;
    }
    let key = setup
        .install_key
        .then(|| {
            VerifiedArtifact::from_bundled_bytes(
                "bundled://docker-fedora-key",
                DOCKER_DNF_KEY,
                DOCKER_DNF_KEY_SHA256,
            )
        })
        .transpose()?;
    let repo = setup
        .install_repo
        .then(|| {
            VerifiedArtifact::from_bundled_bytes(
                "bundled://docker-fedora-repository",
                DOCKER_DNF_REPO,
                DOCKER_DNF_REPO_SHA256,
            )
        })
        .transpose()?;
    Ok((key, repo))
}

pub(crate) fn check_docker_repository_configuration() -> anyhow::Result<()> {
    let setup = docker_repository_bootstrap()?;
    anyhow::ensure!(
        !setup.install_key && !setup.install_repo,
        "Docker Fedora key and repository files are not fully installed"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_fedora_signing_key() -> anyhow::Result<()> {
    let contents = read_required_root_file(Path::new(FEDORA_KEY_PATH))?;
    verify_fedora_key_fingerprint(&contents)
}

#[cfg(target_os = "linux")]
fn read_optional_root_file(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match read_bounded_file(path) {
        Ok((contents, metadata)) => {
            verify_root_file_metadata(&metadata)?;
            Ok(Some(contents))
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn read_required_root_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    let (contents, metadata) = read_bounded_file(path)?;
    verify_root_file_metadata(&metadata)?;
    Ok(contents)
}

#[cfg(target_os = "linux")]
fn verify_root_directory(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o700 == 0o700
            && metadata.mode() & 0o005 == 0o005,
        "DNF configuration directory is not a trusted root-owned directory"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_root_file_metadata(metadata: &std::fs::Metadata) -> anyhow::Result<()> {
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o404 == 0o404,
        "DNF repository or key file is not trusted root-owned data"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_bounded_file(path: &Path) -> anyhow::Result<(Vec<u8>, std::fs::Metadata)> {
    use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt};

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= MAX_FILE_BYTES,
        "DNF repository or key file is not a bounded regular file"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "DNF repository or key file exceeds the size limit"
    );
    Ok((bytes, metadata))
}
