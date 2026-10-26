use std::{
    fs,
    path::{Path, PathBuf},
};
#[cfg(unix)]
use std::{fs::OpenOptions, io::Read};

use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};
use sha2::Digest as _;

#[cfg(test)]
use super::HostPlatform;
use super::{DockerEndpoint, failure};

const MAX_CONTEXT_METADATA_BYTES: u64 = 64 * 1024;

pub(super) fn docker_config_dir(home_dir: Option<&Path>) -> Option<PathBuf> {
    std::env::var_os("DOCKER_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir.map(|home| home.join(".docker")))
}

pub(super) fn linux_rootless_endpoint(
    docker_host: Option<&str>,
    docker_context: Option<&str>,
    uid: Option<u32>,
    config_dir: Option<&Path>,
) -> Result<DockerEndpoint, PrerequisiteFailure> {
    let Some(uid) = uid else {
        return Err(failure(
            PrerequisiteCode::UntrustedEndpoint,
            "The current user ID could not be verified; start the app in the rootless Docker user's session",
            false,
        ));
    };
    if uid == 0 {
        return Err(failure(
            PrerequisiteCode::UntrustedEndpoint,
            "Rootless Docker requires an unprivileged user session; sign in as the Docker user",
            false,
        ));
    }
    let expected_host = format!("unix:///run/user/{uid}/docker.sock");
    if let Some(host) = docker_host.filter(|value| !value.trim().is_empty())
        && host != expected_host
    {
        return Err(failure_owned(
            PrerequisiteCode::UntrustedEndpoint,
            format!(
                "DOCKER_HOST must be unset or exactly {expected_host} for Linux rootless Docker; clear the conflicting DOCKER_HOST value"
            ),
        ));
    }

    if let Some(context) = docker_context.filter(|value| !value.trim().is_empty()) {
        if context != "rootless" {
            return Err(context_conflict(context));
        }
        if let Err(reason) = verify_rootless_context(config_dir, uid, &expected_host) {
            return Err(failure_owned(
                PrerequisiteCode::UntrustedEndpoint,
                format!(
                    "The selected Docker context `rootless` could not be verified for this user's rootless socket ({reason}); clear DOCKER_CONTEXT or rerun dockerd-rootless-setuptool.sh install"
                ),
            ));
        }
    }

    Ok(DockerEndpoint::Unix {
        path: format!("/run/user/{uid}/docker.sock").into(),
        desktop: false,
    })
}

fn context_conflict(context: &str) -> PrerequisiteFailure {
    failure_owned(
        PrerequisiteCode::UntrustedEndpoint,
        format!(
            "Docker context `{context}` is not a verified current-user rootless endpoint; clear DOCKER_CONTEXT or select the verified `rootless` context"
        ),
    )
}

fn failure_owned(code: PrerequisiteCode, message: String) -> PrerequisiteFailure {
    PrerequisiteFailure {
        code,
        message,
        retryable: false,
    }
}

fn verify_rootless_context(
    config_dir: Option<&Path>,
    uid: u32,
    expected_host: &str,
) -> Result<(), &'static str> {
    let config_dir = config_dir.ok_or("Docker config directory is unavailable")?;
    let config_dir =
        fs::canonicalize(config_dir).map_err(|_| "Docker config directory is unavailable")?;
    verify_directory(&config_dir, uid)?;

    let context_dir = config_dir.join("contexts").join("meta").join(context_id());
    verify_directory_chain(&config_dir, &context_dir, uid)?;
    let metadata_path = context_dir.join("meta.json");
    let metadata = read_context_metadata(&metadata_path, uid)?;
    let context_name = metadata
        .get("Name")
        .and_then(serde_json::Value::as_str)
        .ok_or("context metadata has no valid name")?;
    if context_name != "rootless" {
        return Err("context metadata name does not match `rootless`");
    }
    let endpoint_host = metadata
        .get("Endpoints")
        .and_then(|endpoints| endpoints.get("docker"))
        .and_then(|endpoint| endpoint.get("Host"))
        .and_then(serde_json::Value::as_str)
        .ok_or("context metadata has no Docker endpoint")?;
    if endpoint_host != expected_host {
        return Err("context metadata points to a different Docker endpoint");
    }
    Ok(())
}

fn context_id() -> String {
    format!("{:x}", sha2::Sha256::digest(b"rootless"))
}

fn verify_directory_chain(root: &Path, target: &Path, uid: u32) -> Result<(), &'static str> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "context metadata path is outside Docker config")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        verify_directory(&current, uid)?;
    }
    Ok(())
}

#[cfg(unix)]
fn verify_directory(path: &Path, uid: u32) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;

    let metadata =
        fs::symlink_metadata(path).map_err(|_| "context metadata directory is unavailable")?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err("context metadata directory is not owned safely by the current user");
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_directory(_path: &Path, _uid: u32) -> Result<(), &'static str> {
    Err("context metadata ownership cannot be verified on this platform")
}

#[cfg(unix)]
fn read_context_metadata(path: &Path, uid: u32) -> Result<serde_json::Value, &'static str> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let before = fs::symlink_metadata(path).map_err(|_| "context metadata file is unavailable")?;
    if !before.is_file()
        || before.uid() != uid
        || before.mode() & 0o022 != 0
        || before.len() > MAX_CONTEXT_METADATA_BYTES
    {
        return Err("context metadata file is not a safe, bounded regular file");
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "context metadata file could not be opened safely")?;
    let opened = file
        .metadata()
        .map_err(|_| "context metadata file could not be inspected")?;
    if !opened.is_file()
        || opened.uid() != uid
        || opened.mode() & 0o022 != 0
        || opened.len() > MAX_CONTEXT_METADATA_BYTES
        || opened.ino() != before.ino()
        || opened.dev() != before.dev()
    {
        return Err("context metadata file changed while it was being verified");
    }
    let mut contents = Vec::with_capacity(opened.len() as usize);
    file.take(MAX_CONTEXT_METADATA_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|_| "context metadata file could not be read")?;
    if contents.len() as u64 > MAX_CONTEXT_METADATA_BYTES {
        return Err("context metadata file exceeds the size limit");
    }
    serde_json::from_slice(&contents).map_err(|_| "context metadata is not valid JSON")
}

#[cfg(not(unix))]
fn read_context_metadata(_path: &Path, _uid: u32) -> Result<serde_json::Value, &'static str> {
    Err("context metadata ownership cannot be verified on this platform")
}

#[cfg(test)]
#[path = "context/tests.rs"]
mod tests;
