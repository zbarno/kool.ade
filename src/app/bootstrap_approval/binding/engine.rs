#[cfg(target_os = "linux")]
use crate::app::bootstrap_repair_policy::Host;
use crate::app::bootstrap_repair_policy::{Operation, Proposal};
use crate::execution::docker::images::RuntimeProfile;
#[cfg(target_os = "linux")]
use crate::execution::docker::{
    engine::{DockerEndpoint, EngineDiscovery, HostPlatform},
    images::ImageLock,
};

use super::MachineBinding;

pub(in crate::app::bootstrap_approval) struct VerifiedEngine {
    binding: MachineBinding,
    #[cfg(target_os = "linux")]
    engine: EngineDiscovery,
    #[cfg(target_os = "linux")]
    endpoint_fingerprint: String,
}

impl VerifiedEngine {
    pub(in crate::app::bootstrap_approval) async fn current(
        proposal: &Proposal,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            proposal.operation == Operation::PrepareLockedImage,
            "No native repair operation is enabled on this platform yet"
        );
        anyhow::ensure!(
            proposal.host == super::current_host(),
            "Repair proposal does not match the current host"
        );
        #[cfg(target_os = "linux")]
        {
            let endpoint =
                DockerEndpoint::from_process_environment(HostPlatform::Linux).map_err(|error| {
                    anyhow::anyhow!("Trusted Docker endpoint could not be verified: {error}")
                })?;
            anyhow::ensure!(
                !endpoint.is_desktop(),
                "Approved image preparation is not enabled for Docker Desktop yet"
            );
            let engine = EngineDiscovery::connect_pinned(endpoint.clone()).map_err(|error| {
                anyhow::anyhow!("Trusted Docker Engine could not be verified: {error}")
            })?;
            let snapshot = engine
                .inspect_snapshot()
                .await
                .map_err(|error| anyhow::anyhow!("Docker probe failed: {error}"))?;
            anyhow::ensure!(
                snapshot.native_linux
                    && snapshot.rootless
                    && !snapshot.docker_desktop
                    && !snapshot.daemon_id.trim().is_empty(),
                "A verified native rootless Docker Engine is required"
            );
            let profile = match proposal.image_profile.as_deref() {
                Some("general") => RuntimeProfile::General,
                Some("rust") => RuntimeProfile::Rust,
                Some("node") => RuntimeProfile::Node,
                Some("python") => RuntimeProfile::Python,
                Some("go") => RuntimeProfile::Go,
                _ => anyhow::bail!("The requested image profile is not approved"),
            };
            let lock = ImageLock::bundled()?;
            let image = lock.select(profile, &snapshot.architecture)?;
            let estimate = lock.size_estimate(profile, &snapshot.architecture)?;
            let disk_available_bytes = snapshot.disk_available_bytes.ok_or_else(|| {
                anyhow::anyhow!("Docker storage capacity is unknown; image preparation is disabled")
            })?;
            let minimum_required_bytes = snapshot
                .minimum_disk_bytes
                .checked_add(estimate.estimated_storage_bytes)
                .ok_or_else(|| anyhow::anyhow!("Image storage requirement overflowed"))?;
            anyhow::ensure!(
                disk_available_bytes >= minimum_required_bytes,
                "Docker image storage does not have enough free space for this pinned runtime image"
            );
            let profile_name = proposal
                .image_profile
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("The repair proposal has no image profile"))?;
            let engine_identity_fingerprint = super::super::engine_identity_fingerprint(
                &endpoint.fingerprint(),
                &snapshot.daemon_id,
            )
            .ok_or_else(|| {
                anyhow::anyhow!("The trusted engine identity could not be fingerprinted")
            })?;
            anyhow::ensure!(
                proposal.prerequisite_id
                    == format!(
                        "setup:images:{profile_name}:{}:{engine_identity_fingerprint}",
                        image.platform.trim_start_matches("linux/")
                    ),
                "Repair proposal does not match the approved image architecture and engine identity"
            );
            let machine_id = super::linux_machine_id()?;
            let principal = unsafe { libc::geteuid() };
            // Disk availability and probe time are freshness evidence, not
            // stable daemon identity, so neither is included in this hash.
            let probe_hash = crate::execution::docker::engine::approval_fingerprint(
                &endpoint, &snapshot, &image,
            )?;
            let endpoint_fingerprint = endpoint.fingerprint();
            let mut binding = MachineBinding::from_facts(
                Host::Linux,
                &machine_id,
                principal.to_string().as_str(),
                &probe_hash,
                super::super::now_unix_ms()?,
            )?;
            binding.daemon_id = snapshot.daemon_id.clone();
            binding.engine_identity_fingerprint = engine_identity_fingerprint;
            binding.daemon_version = snapshot.daemon_version.clone();
            binding.image_reference.clone_from(&image.reference);
            binding.image_digest.clone_from(&image.digest);
            binding.image_platform.clone_from(&image.platform);
            binding.estimated_pull_bytes = estimate.estimated_pull_bytes;
            binding.estimated_storage_bytes = estimate.estimated_storage_bytes;
            binding.disk_available_bytes = disk_available_bytes;
            binding.minimum_disk_bytes = snapshot.minimum_disk_bytes;
            Ok(Self {
                binding,
                engine,
                endpoint_fingerprint,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!(
                "Native machine approval is disabled until this platform has a verified trusted endpoint"
            )
        }
    }

    pub(in crate::app::bootstrap_approval) fn binding(&self) -> &MachineBinding {
        &self.binding
    }

    pub(in crate::app::bootstrap_approval) fn into_binding(self) -> MachineBinding {
        self.binding
    }

    pub(in crate::app::bootstrap_approval) async fn run_smoke_test(
        self,
        _authorization: super::super::capability::ApprovedAction,
        _pull_control: &crate::execution::docker::engine::ImagePullControl,
    ) -> anyhow::Result<super::super::capability::ImagePreparationReceipt> {
        #[cfg(target_os = "linux")]
        {
            let expected_profile = approved_runtime_profile(_authorization.image_profile())
                .ok_or_else(|| anyhow::anyhow!("The approved runtime image profile is invalid"))?;
            let staging = crate::execution::docker::environment::app_staging_root()?;
            let receipt = self
                .engine
                .test_approved_image(&staging, _authorization, _pull_control)
                .await
                .map_err(anyhow::Error::new)?;
            anyhow::ensure!(
                receipt.profile == expected_profile
                    && receipt.image_digest == self.binding.image_digest
                    && receipt.architecture
                        == self.binding.image_platform.trim_start_matches("linux/")
                    && receipt.engine_identity_fingerprint
                        == self.binding.engine_identity_fingerprint,
                "Approved image smoke receipt changed during verified preparation"
            );
            let image = ImageLock::bundled()?.select(
                expected_profile,
                self.binding.image_platform.trim_start_matches("linux/"),
            )?;
            anyhow::ensure!(
                image.reference == self.binding.image_reference
                    && image.digest == self.binding.image_digest
                    && image.platform == self.binding.image_platform,
                "Approved image changed during verified preparation"
            );
            if requires_rootless_readiness_receipt(expected_profile) {
                let daemon_fingerprint = {
                    use sha2::Digest as _;
                    format!(
                        "{:x}",
                        sha2::Sha256::digest(self.binding.daemon_id.as_bytes())
                    )
                };
                crate::persistence::linux_rootless_smoke_receipt::record(
                    &self.endpoint_fingerprint,
                    &daemon_fingerprint,
                    &self.binding.daemon_version,
                    &image,
                    crate::persistence::harness_settings::current_unix_ms(),
                )?;
            }
            Ok(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!("Approved image preparation is unavailable on this platform")
        }
    }
}

fn approved_runtime_profile(profile: Option<&str>) -> Option<RuntimeProfile> {
    Some(match profile? {
        "general" => RuntimeProfile::General,
        "rust" => RuntimeProfile::Rust,
        "node" => RuntimeProfile::Node,
        "python" => RuntimeProfile::Python,
        "go" => RuntimeProfile::Go,
        _ => return None,
    })
}

fn requires_rootless_readiness_receipt(profile: RuntimeProfile) -> bool {
    profile == RuntimeProfile::General
}

#[cfg(test)]
#[path = "engine/tests.rs"]
mod tests;
