use super::{DockerEnvironmentStatus, probe, process_endpoint};

impl DockerEnvironmentStatus {
    pub(crate) fn readiness_fingerprint(&self) -> String {
        use sha2::Digest as _;

        let mut available_images = self
            .available_images
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        available_images.sort_unstable();
        available_images.dedup();
        let binding = (
            self.state,
            self.engine_ready,
            self.desktop_vm,
            self.desktop_transport_verified,
            self.desktop_vm_capacity_verified,
            self.workspace_staging_available,
            &self.daemon_version,
            &self.architecture,
            self.rootless,
            self.cpu_limits,
            self.memory_limits,
            self.pids_limit,
            self.cgroups_supported,
            available_images,
            self.disk_available_bytes,
            &self.daemon_identity_fingerprint,
        );
        let encoded = serde_json::to_vec(&binding).expect("Docker readiness facts serialize");
        format!("{:x}", sha2::Sha256::digest(encoded))
    }

    /// Stable identity for image task IDs and receipts. The daemon ID is
    /// already reduced to a hash by the probe; readiness and image inventory
    /// are intentionally excluded so a pull does not change the task key.
    pub(crate) fn engine_identity_fingerprint(&self) -> Option<String> {
        use sha2::Digest as _;

        if !self.engine_ready && !self.desktop_capacity_bootstrap_identity_allowed() {
            return None;
        }
        let endpoint = process_endpoint(probe::host_platform()).ok()?.fingerprint();
        let daemon = self.daemon_identity_fingerprint.as_deref()?;
        if !is_sha256_hex(&endpoint) || !is_sha256_hex(daemon) {
            return None;
        }
        let identity =
            serde_json::to_vec(&("koolade-engine-identity-v1", endpoint, daemon)).ok()?;
        Some(format!("{:x}", sha2::Sha256::digest(identity)))
    }

    pub(crate) fn current_endpoint_fingerprint() -> Option<String> {
        let endpoint = process_endpoint(probe::host_platform()).ok()?;
        Some(probe::endpoint_fingerprint(
            &endpoint,
            std::env::var("DOCKER_CONTEXT").ok().as_deref(),
            std::env::var("DOCKER_HOST").ok().as_deref(),
        ))
    }

    /// Stable normalized endpoint identity, independent of shell overrides
    /// that resolve to the same trusted local socket.
    pub(crate) fn current_endpoint_identity_fingerprint() -> Option<String> {
        let endpoint = process_endpoint(probe::host_platform()).ok()?;
        Some(endpoint.fingerprint())
    }

    fn desktop_capacity_bootstrap_identity_allowed(&self) -> bool {
        !self.engine_ready
            && self.state == super::DockerEnvironmentState::InsufficientDisk
            && self.desktop_vm
            && self.desktop_transport_verified
            && !self.desktop_vm_capacity_verified
            && self.workspace_staging_available
            && self.disk_available_bytes.is_none()
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::DockerEnvironmentStatus;
    use crate::execution::docker::{
        discovery::{PrerequisiteCode, PrerequisiteFailure},
        environment::DockerEnvironmentState,
    };

    fn capacity_bootstrap_status() -> DockerEnvironmentStatus {
        let mut status = DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: PrerequisiteCode::InsufficientDisk,
                message: "VM capacity is not available yet".into(),
                retryable: true,
            },
            true,
        );
        status.desktop_vm = true;
        status.desktop_transport_verified = true;
        status.workspace_staging_available = true;
        status
    }

    #[test]
    fn engine_identity_is_available_only_for_the_desktop_capacity_bootstrap() {
        let status = capacity_bootstrap_status();
        assert!(status.desktop_capacity_bootstrap_identity_allowed());

        let mut unavailable_workspace = status.clone();
        unavailable_workspace.workspace_staging_available = false;
        assert!(!unavailable_workspace.desktop_capacity_bootstrap_identity_allowed());

        let mut unverified_transport = status.clone();
        unverified_transport.desktop_transport_verified = false;
        assert!(!unverified_transport.desktop_capacity_bootstrap_identity_allowed());

        let mut capacity_present = status.clone();
        capacity_present.desktop_vm_capacity_verified = true;
        assert!(!capacity_present.desktop_capacity_bootstrap_identity_allowed());

        let mut other_failure = status.clone();
        other_failure.state = DockerEnvironmentState::CapabilitiesIncompatible;
        assert!(!other_failure.desktop_capacity_bootstrap_identity_allowed());

        let mut disk_present = status;
        disk_present.disk_available_bytes = Some(1_000_000_000);
        assert!(!disk_present.desktop_capacity_bootstrap_identity_allowed());
    }
}
