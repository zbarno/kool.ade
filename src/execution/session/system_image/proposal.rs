use super::*;

impl ExecutionSession {
    pub fn cache_target_arch(&self) -> Option<String> {
        if let Ok(derived) = self.derived_image.lock()
            && let Some(derived) = derived.as_ref()
        {
            return derived
                .target_platform
                .strip_prefix("linux/")
                .map(str::to_owned);
        }
        let digest = self.image_digest.read().ok()?.clone();
        crate::execution::docker::images::ImageLock::bundled()
            .ok()?
            .profiles
            .values()
            .flatten()
            .find(|image| image.digest == digest)
            .and_then(|image| image.platform.strip_prefix("linux/").map(str::to_owned))
    }

    pub(super) fn system_proposal(
        &self,
        packages: &[String],
        purpose: &str,
    ) -> anyhow::Result<crate::execution::docker::image_extension::SystemImageExtensionRequest>
    {
        let architecture = self
            .cache_target_arch()
            .ok_or_else(|| anyhow::anyhow!("selected base image architecture is unavailable"))?;
        let digest = self
            .selected_image_digest()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let lock = crate::execution::docker::images::ImageLock::bundled()
            .map_err(|error| anyhow::anyhow!("{}", error.message))?;
        let profiles = [
            (
                "general",
                crate::execution::docker::images::RuntimeProfile::General,
            ),
            (
                "rust",
                crate::execution::docker::images::RuntimeProfile::Rust,
            ),
            (
                "node",
                crate::execution::docker::images::RuntimeProfile::Node,
            ),
            (
                "python",
                crate::execution::docker::images::RuntimeProfile::Python,
            ),
            ("go", crate::execution::docker::images::RuntimeProfile::Go),
        ];
        let base_profile = profiles
            .into_iter()
            .find_map(|(key, profile)| {
                lock.profiles
                    .get(key)?
                    .iter()
                    .find(|variant| variant.digest == digest)
                    .map(|_| profile)
            })
            .ok_or_else(|| {
                anyhow::anyhow!("selected base image is not an approved implementation profile")
            })?;
        Ok(crate::execution::docker::image_extension::SystemImageExtensionRequest {
            task_id: self.workspace.task_id.clone(),
            project_id: self.workspace.project_id.clone(),
            distribution: crate::execution::docker::image_extension::LinuxDistribution::DebianBookworm,
            base_profile,
            base_image_digest: digest,
            target_arch: architecture,
            packages: packages.to_vec(),
            trust: crate::execution::docker::image_extension::PackageTrust::ApprovedDistributionRepository,
            purpose: purpose.to_owned(),
        })
    }

    /// Resolve the exact signed repository closure before the user decision.
    pub(crate) fn resolve_bookworm_package_manifest(
        &self,
        proposal: &crate::execution::docker::image_extension::SystemImageExtensionRequest,
    ) -> Result<
        Vec<crate::execution::docker::image_extension::builder::PackageArtifactIdentity>,
        ExecutionError,
    > {
        self.verify_authorization()?;
        if self.busy.load(Ordering::Acquire) || proposal.task_id != self.workspace.task_id {
            return Err(policy_error(
                "System package proposal is outside the idle task session scope",
            ));
        }
        let base = proposal
            .resolve_base_selection(&self.workspace.task_id, &self.workspace.project_id)
            .map_err(|error| {
                policy_error(&format!("System package proposal was rejected: {error:#}"))
            })?;
        if base.digest != self.selected_image_digest()? {
            return Err(policy_error(
                "System package proposal does not match the selected base image",
            ));
        }
        let installed_base = self
            .runtime
            .block_on(self.backend.inspect_system_package_state(&base))?;
        let architecture = base
            .platform
            .strip_prefix("linux/")
            .ok_or_else(|| policy_error("System package target platform is invalid"))?;
        let plan = crate::harness::resource_bridge::RepositoryPackagePlan::resolve(
            &proposal.packages,
            &installed_base,
            architecture,
        )
        .map_err(|error| {
            policy_error(&format!(
                "Unable to resolve signed Debian package closure: {error:#}"
            ))
        })?;
        Ok(plan.identities())
    }
}
