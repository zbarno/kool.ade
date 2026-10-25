#[cfg(target_os = "linux")]
use crate::app::bootstrap_repair_policy::Host;
use crate::app::bootstrap_repair_policy::{Operation, Proposal};
#[cfg(target_os = "linux")]
use crate::execution::docker::{
    engine::{DockerEndpoint, EngineDiscovery, HostPlatform},
    images::{ImageLock, RuntimeProfile},
};

use super::MachineBinding;

pub(in crate::app::bootstrap_approval) struct VerifiedEngine {
    binding: MachineBinding,
    #[cfg(target_os = "linux")]
    engine: EngineDiscovery,
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
            let image = ImageLock::bundled()?.select(profile, &snapshot.architecture)?;
            anyhow::ensure!(
                proposal.prerequisite_id
                    == format!(
                        "setup:images:{}:{}",
                        proposal.image_profile.as_deref().unwrap(),
                        image.platform.trim_start_matches("linux/")
                    ),
                "Repair proposal does not match the approved image architecture"
            );
            let machine_id = super::linux_machine_id()?;
            let principal = unsafe { libc::geteuid() };
            // Disk availability and probe time are freshness evidence, not
            // stable daemon identity, so neither is included in this hash.
            let probe_hash = crate::execution::docker::engine::approval_fingerprint(
                &endpoint, &snapshot, &image,
            )?;
            let mut binding = MachineBinding::from_facts(
                Host::Linux,
                &machine_id,
                principal.to_string().as_str(),
                &probe_hash,
                super::super::now_unix_ms()?,
            )?;
            binding.daemon_id = snapshot.daemon_id.clone();
            binding.image_reference.clone_from(&image.reference);
            binding.image_digest.clone_from(&image.digest);
            binding.image_platform.clone_from(&image.platform);
            Ok(Self { binding, engine })
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
        _authorization: &super::super::capability::ApprovedAction,
    ) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        {
            let staging = crate::execution::docker::environment::app_staging_root()?;
            self.engine
                .test_approved_image(&staging, _authorization)
                .await?;
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!("Approved image preparation is unavailable on this platform")
        }
    }
}
