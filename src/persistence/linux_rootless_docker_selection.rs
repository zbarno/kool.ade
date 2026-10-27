//! Operator-local identity for the verified Linux rootless Docker endpoint.
//!
//! The record contains no socket path, environment override, daemon ID, or
//! credential. The canonical socket can be derived from the stored UID.

use serde::{Deserialize, Serialize};
use std::path::Path;

const FILE: &str = "linux-rootless-docker-selection.json";
const SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct LinuxRootlessDockerSelectionV1 {
    schema_version: u8,
    user_id: u32,
    socket_owner_id: u32,
    endpoint_fingerprint: String,
    daemon_fingerprint: String,
}

impl LinuxRootlessDockerSelectionV1 {
    fn new(
        user_id: u32,
        socket_owner_id: u32,
        endpoint_fingerprint: &str,
        daemon_fingerprint: &str,
    ) -> anyhow::Result<Self> {
        let value = Self {
            schema_version: SCHEMA_VERSION,
            user_id,
            socket_owner_id,
            endpoint_fingerprint: endpoint_fingerprint.to_owned(),
            daemon_fingerprint: daemon_fingerprint.to_owned(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == SCHEMA_VERSION,
            "unsupported Linux Docker selection version"
        );
        anyhow::ensure!(
            self.user_id != 0 && self.socket_owner_id == self.user_id,
            "Linux rootless Docker socket is not owned by the current user"
        );
        anyhow::ensure!(
            valid_fingerprint(&self.endpoint_fingerprint)
                && valid_fingerprint(&self.daemon_fingerprint),
            "invalid Linux Docker identity fingerprint"
        );
        Ok(())
    }

    fn matches_observation(&self, observation: &Observation<'_>) -> bool {
        self.user_id == observation.user_id
            && match observation.socket {
                SocketObservation::Owned(owner) => owner == self.user_id,
                SocketObservation::Missing => {
                    observation.rootless.is_none() && observation.daemon_fingerprint.is_none()
                }
                SocketObservation::Unknown => false,
            }
            && observation
                .endpoint_fingerprint
                .is_some_and(|fingerprint| fingerprint == self.endpoint_fingerprint)
            && observation.rootless != Some(false)
            && observation
                .daemon_fingerprint
                .is_none_or(|fingerprint| fingerprint == self.daemon_fingerprint)
    }
}

struct Observation<'a> {
    user_id: u32,
    socket: SocketObservation,
    endpoint_fingerprint: Option<&'a str>,
    daemon_fingerprint: Option<&'a str>,
    rootless: Option<bool>,
}

#[derive(Clone, Copy)]
enum SocketObservation {
    Owned(u32),
    Missing,
    Unknown,
}

pub(crate) fn settings_path() -> std::path::PathBuf {
    crate::persistence::state_root().join(FILE)
}

/// Record a verified current-user rootless endpoint, or discard stale identity.
/// A temporarily stopped daemon preserves a matching selection while its
/// daemon fingerprint cannot be re-observed.
pub(crate) fn observe_current(
    endpoint_fingerprint: Option<&str>,
    daemon_fingerprint: Option<&str>,
    rootless: Option<bool>,
) -> anyhow::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        let user_id = unsafe { libc::geteuid() };
        let socket = current_socket_observation(user_id);
        let endpoint_fingerprint =
            endpoint_fingerprint.and_then(|_| current_normalized_endpoint_fingerprint());
        reconcile_at(
            &settings_path(),
            Observation {
                user_id,
                socket,
                endpoint_fingerprint: endpoint_fingerprint.as_deref(),
                daemon_fingerprint,
                rootless,
            },
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (endpoint_fingerprint, daemon_fingerprint, rootless);
        Ok(false)
    }
}

#[cfg(target_os = "linux")]
fn current_normalized_endpoint_fingerprint() -> Option<String> {
    use crate::execution::docker::engine::HostPlatform;

    let endpoint = crate::execution::docker::engine::DockerEndpoint::from_process_environment(
        HostPlatform::Linux,
    )
    .ok()?;
    Some(endpoint.fingerprint())
}

#[cfg(target_os = "linux")]
fn current_socket_observation(user_id: u32) -> SocketObservation {
    if user_id == 0 {
        return SocketObservation::Unknown;
    }
    let runtime_dir = std::path::PathBuf::from(format!("/run/user/{user_id}"));
    socket_observation_at(&runtime_dir, &runtime_dir.join("docker.sock"), user_id)
}

#[cfg(target_os = "linux")]
fn socket_observation_at(runtime_dir: &Path, socket: &Path, user_id: u32) -> SocketObservation {
    use crate::execution::docker::engine::endpoint::socket::{
        RootlessSocketObservation, observe_rootless_socket_at,
    };

    match observe_rootless_socket_at(runtime_dir, socket, user_id) {
        RootlessSocketObservation::Owned(owner) => SocketObservation::Owned(owner),
        RootlessSocketObservation::Missing => SocketObservation::Missing,
        RootlessSocketObservation::Unknown => SocketObservation::Unknown,
    }
}

fn reconcile_at(path: &Path, observation: Observation<'_>) -> anyhow::Result<bool> {
    let (stored, load_error) = load_from(path);
    let mut stored_matches = stored
        .as_ref()
        .is_some_and(|selection| selection.matches_observation(&observation));
    if load_error.is_some() {
        clear_at(path)?;
        stored_matches = false;
    } else if stored.is_some() && !stored_matches {
        clear_at(path)?;
    }

    if observation.rootless == Some(true)
        && let (Some(owner), Some(endpoint), Some(daemon)) = (
            match observation.socket {
                SocketObservation::Owned(owner) => Some(owner),
                SocketObservation::Missing | SocketObservation::Unknown => None,
            },
            observation.endpoint_fingerprint,
            observation.daemon_fingerprint,
        )
        && owner == observation.user_id
    {
        let selection =
            LinuxRootlessDockerSelectionV1::new(observation.user_id, owner, endpoint, daemon)?;
        if stored.as_ref() != Some(&selection) {
            save_to(path, &selection)?;
        }
        return Ok(true);
    }
    Ok(stored_matches)
}

fn load_from(path: &Path) -> (Option<LinuxRootlessDockerSelectionV1>, Option<String>) {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<LinuxRootlessDockerSelectionV1>(&bytes) {
            Ok(selection) => match selection.validate() {
                Ok(()) => (Some(selection), None),
                Err(error) => (
                    None,
                    Some(format!("Invalid Linux Docker selection: {error}")),
                ),
            },
            Err(error) => (
                None,
                Some(format!("Could not read Linux Docker selection: {error}")),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
        Err(error) => (
            None,
            Some(format!("Could not read Linux Docker selection: {error}")),
        ),
    }
}

fn save_to(path: &Path, selection: &LinuxRootlessDockerSelectionV1) -> anyhow::Result<()> {
    selection.validate()?;
    crate::artifacts::atomic_write(path, &serde_json::to_string_pretty(selection)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn clear_at(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
#[path = "linux_rootless_docker_selection/tests.rs"]
mod tests;
