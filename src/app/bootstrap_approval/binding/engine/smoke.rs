#[cfg(any(target_os = "linux", test))]
use crate::app::bootstrap_repair_policy::Host;
use crate::execution::docker::images::{ImageLock, RuntimeProfile};

use super::VerifiedEngine;

impl VerifiedEngine {
    pub(in crate::app::bootstrap_approval) async fn run_smoke_test(
        self,
        authorization: super::super::super::capability::ApprovedAction,
        pull_control: &crate::execution::docker::engine::ImagePullControl,
    ) -> anyhow::Result<super::super::super::capability::ImagePreparationReceipt> {
        let expected_profile = approved_runtime_profile(authorization.image_profile())
            .ok_or_else(|| anyhow::anyhow!("The approved runtime image profile is invalid"))?;
        anyhow::ensure!(
            authorization.capacity_probe_required() == self.binding.capacity_probe_required,
            "Approved image capacity policy changed before preparation"
        );
        let staging = crate::execution::docker::environment::app_staging_root()?;
        let receipt = self
            .engine
            .test_approved_image(&staging, authorization, pull_control)
            .await
            .map_err(anyhow::Error::new)?;
        anyhow::ensure!(
            receipt.profile == expected_profile
                && receipt.image_digest == self.binding.image_digest
                && receipt.architecture == self.binding.image_platform.trim_start_matches("linux/")
                && receipt.engine_identity_fingerprint == self.binding.engine_identity_fingerprint,
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
        #[cfg(target_os = "linux")]
        if requires_rootless_readiness_receipt(expected_profile, self.binding.host) {
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
}

pub(super) fn approved_runtime_profile(profile: Option<&str>) -> Option<RuntimeProfile> {
    Some(match profile? {
        "general" => RuntimeProfile::General,
        "rust" => RuntimeProfile::Rust,
        "node" => RuntimeProfile::Node,
        "python" => RuntimeProfile::Python,
        "go" => RuntimeProfile::Go,
        _ => return None,
    })
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn requires_rootless_readiness_receipt(profile: RuntimeProfile, host: Host) -> bool {
    host == Host::Linux && profile == RuntimeProfile::General
}
