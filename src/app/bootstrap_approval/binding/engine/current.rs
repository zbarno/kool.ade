use crate::app::bootstrap_repair_policy::{Host, Proposal};
use crate::execution::docker::{
    engine::{DockerEndpoint, EngineDiscovery, EngineSnapshot, HostPlatform},
    images::{
        DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES, DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES, ImageLock,
        RuntimeProfile,
    },
};

use super::{super::MachineBinding, VerifiedEngine};

impl VerifiedEngine {
    pub(in crate::app::bootstrap_approval) async fn current(
        proposal: &Proposal,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            proposal.operation
                == crate::app::bootstrap_repair_policy::Operation::PrepareLockedImage,
            "No native repair operation is enabled on this platform yet"
        );
        let host = super::super::current_host();
        anyhow::ensure!(
            proposal.host == host,
            "Repair proposal does not match the current host"
        );

        let endpoint = process_endpoint()?;
        let desktop = matches!(host, Host::MacOs | Host::Windows);
        anyhow::ensure!(
            endpoint.is_desktop() == desktop,
            "Only the current host's verified local Docker endpoint is supported"
        );
        let engine = connect_endpoint(endpoint.clone())?;
        let profile = super::smoke::approved_runtime_profile(proposal.image_profile.as_deref())
            .ok_or_else(|| anyhow::anyhow!("The requested image profile is not approved"))?;
        let snapshot = engine
            .inspect_snapshot_for_image_preparation(profile)
            .await
            .map_err(|error| anyhow::anyhow!("Docker probe failed: {error}"))?;
        validate_engine(&snapshot, host, desktop)?;

        let lock = ImageLock::bundled()?;
        let image = lock.select(profile, &snapshot.architecture)?;
        let estimate = lock.size_estimate(profile, &snapshot.architecture)?;
        let capacity_probe_required = desktop
            && profile == RuntimeProfile::General
            && snapshot.disk_available_bytes.is_none();
        if capacity_probe_required {
            anyhow::ensure!(
                estimate.estimated_pull_bytes <= DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES
                    && estimate.estimated_storage_bytes <= DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES,
                "The pinned General image exceeds the bounded Docker Desktop bootstrap limit"
            );
        }
        let required_bytes = snapshot
            .minimum_disk_bytes
            .checked_add(estimate.estimated_storage_bytes)
            .ok_or_else(|| anyhow::anyhow!("Image storage requirement overflowed"))?;
        if !capacity_probe_required {
            let available = snapshot.disk_available_bytes.ok_or_else(|| {
                anyhow::anyhow!("Docker storage capacity is unknown; image preparation is disabled")
            })?;
            anyhow::ensure!(
                available >= required_bytes,
                "Docker image storage does not have enough free space for this pinned runtime image"
            );
        }

        let profile_name = image_profile_name(profile);
        let architecture = image
            .platform
            .strip_prefix("linux/")
            .ok_or_else(|| anyhow::anyhow!("The approved image platform is invalid"))?;
        let endpoint_fingerprint = endpoint.fingerprint();
        let engine_identity_fingerprint =
            crate::app::bootstrap_approval::engine_identity_fingerprint(
                &endpoint_fingerprint,
                &snapshot.daemon_id,
            )
            .ok_or_else(|| {
                anyhow::anyhow!("The trusted engine identity could not be fingerprinted")
            })?;
        anyhow::ensure!(
            proposal.prerequisite_id
                == format!(
                    "setup:images:{profile_name}:{architecture}:{engine_identity_fingerprint}"
                ),
            "Repair proposal does not match the approved image architecture and engine identity"
        );
        let probe_hash =
            crate::execution::docker::engine::approval_fingerprint(&endpoint, &snapshot, &image)?;
        let mut binding = host_binding(host, &probe_hash, super::super::super::now_unix_ms()?)?;
        binding.engine_identity_fingerprint = engine_identity_fingerprint;
        binding.daemon_id = snapshot.daemon_id.clone();
        binding.daemon_version = snapshot.daemon_version.clone();
        binding.image_profile = profile_name.to_owned();
        binding.image_reference.clone_from(&image.reference);
        binding.image_digest.clone_from(&image.digest);
        binding.image_platform.clone_from(&image.platform);
        binding.estimated_pull_bytes = estimate.estimated_pull_bytes;
        binding.estimated_storage_bytes = estimate.estimated_storage_bytes;
        binding.disk_available_bytes = snapshot.disk_available_bytes;
        binding.minimum_disk_bytes = snapshot.minimum_disk_bytes;
        binding.capacity_probe_required = capacity_probe_required;

        Ok(Self {
            binding,
            engine,
            #[cfg(target_os = "linux")]
            endpoint_fingerprint,
        })
    }
}

fn validate_engine(snapshot: &EngineSnapshot, host: Host, desktop: bool) -> anyhow::Result<()> {
    anyhow::ensure!(
        snapshot.os_type.eq_ignore_ascii_case("linux")
            && matches!(snapshot.architecture.as_str(), "amd64" | "arm64")
            && !snapshot.daemon_id.trim().is_empty()
            && snapshot.docker_desktop == desktop,
        "A verified local Linux-container Docker Engine is required"
    );
    if host == Host::Linux {
        anyhow::ensure!(
            snapshot.native_linux && snapshot.rootless,
            "A verified native rootless Docker Engine is required"
        );
    } else {
        anyhow::ensure!(
            !snapshot.native_linux && desktop,
            "A verified Docker Desktop Linux VM is required"
        );
    }
    Ok(())
}

fn process_endpoint() -> anyhow::Result<DockerEndpoint> {
    #[cfg(target_os = "linux")]
    {
        #[cfg(test)]
        let endpoint =
            DockerEndpoint::from_process_environment_with_test_support(HostPlatform::Linux);
        #[cfg(not(test))]
        let endpoint = DockerEndpoint::from_process_environment(HostPlatform::Linux);
        endpoint.map_err(anyhow::Error::new)
    }
    #[cfg(target_os = "macos")]
    {
        DockerEndpoint::from_process_environment(HostPlatform::MacOs).map_err(anyhow::Error::new)
    }
    #[cfg(target_os = "windows")]
    {
        DockerEndpoint::from_process_environment(HostPlatform::Windows).map_err(anyhow::Error::new)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("Docker image preparation is unsupported on this host")
    }
}

fn connect_endpoint(endpoint: DockerEndpoint) -> anyhow::Result<EngineDiscovery> {
    #[cfg(unix)]
    {
        EngineDiscovery::connect_pinned(endpoint).map_err(anyhow::Error::new)
    }
    #[cfg(windows)]
    {
        EngineDiscovery::connect(endpoint).map_err(anyhow::Error::new)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = endpoint;
        anyhow::bail!("Docker image preparation is unsupported on this host")
    }
}

fn host_binding(host: Host, probe_hash: &str, probed_at: u64) -> anyhow::Result<MachineBinding> {
    #[cfg(target_os = "linux")]
    {
        anyhow::ensure!(
            host == Host::Linux,
            "Repair proposal does not match the current host"
        );
        let machine_id = super::super::linux_machine_id()?;
        let principal = unsafe { libc::geteuid() };
        MachineBinding::from_facts(
            host,
            &machine_id,
            principal.to_string().as_str(),
            probe_hash,
            probed_at,
        )
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let identity = crate::app::bootstrap_remediation::HostIdentity::current()?;
        anyhow::ensure!(
            identity.host() == host,
            "Native machine identity does not match the current host"
        );
        MachineBinding::from_host_identity(&identity, probe_hash, probed_at)
    }
}

fn image_profile_name(profile: RuntimeProfile) -> &'static str {
    match profile {
        RuntimeProfile::General => "general",
        RuntimeProfile::Rust => "rust",
        RuntimeProfile::Node => "node",
        RuntimeProfile::Python => "python",
        RuntimeProfile::Go => "go",
        RuntimeProfile::BrowserQa | RuntimeProfile::DesktopQa => "unsupported",
    }
}
