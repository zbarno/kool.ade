use super::*;

mod proposal;

impl ExecutionSession {
    pub(crate) fn resolve_system_dependency_manifest(
        &self,
        need: &crate::harness::DependencyNeed,
    ) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
        anyhow::ensure!(
            need.ecosystem == crate::harness::PackageEcosystem::System
                && need.kind == crate::harness::DependencyKind::SystemTool,
            "request is not a structured system package need"
        );
        let package = need
            .package
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("one exact Debian package name is required"))?;
        anyhow::ensure!(
            need.source
                .as_deref()
                .is_none_or(|source| source == "https://deb.debian.org/debian"
                    || source == "debian:bookworm"),
            "only the approved Debian Bookworm archive is supported"
        );
        let packages = crate::harness::resource_bridge::system_install_packages(
            &need.command,
            package,
            need.version.as_deref(),
        )
        .ok_or_else(|| anyhow::anyhow!("system package request is not a bounded apt install"))?;
        let proposal = self.system_proposal(&packages, &need.reason)?;
        let manifest = self
            .resolve_bookworm_package_manifest(&proposal)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        if let Some(version) = need.version.as_deref() {
            anyhow::ensure!(
                manifest
                    .iter()
                    .any(|identity| identity.name == package && identity.version == version),
                "requested Debian version is not the signed repository candidate"
            );
        }
        anyhow::ensure!(
            packages
                .iter()
                .all(|package| manifest.iter().any(|identity| identity.name == *package)),
            "signed package closure omitted a requested Debian package"
        );
        Ok(manifest
            .into_iter()
            .map(|identity| crate::harness::DependencyPackageIdentity {
                package: identity.name,
                version: identity.version,
                source: identity.source,
                integrity: format!("sha256:{}", identity.sha256),
            })
            .collect())
    }

    pub(crate) fn prepare_authorized_system_dependency(
        &self,
        need: &crate::harness::DependencyNeed,
        decision: crate::harness::DependencyDecision,
    ) -> Result<crate::execution::docker::image_extension::DerivedImage, ExecutionError> {
        if decision != crate::harness::DependencyDecision::UserAuthorizeForTask {
            return Err(policy_error(
                "System image derivation requires explicit task-scoped user approval",
            ));
        }
        if need.ecosystem != crate::harness::PackageEcosystem::System
            || need.kind != crate::harness::DependencyKind::SystemTool
            || need.source.as_deref().is_some_and(|source| {
                source != "https://deb.debian.org/debian" && source != "debian:bookworm"
            })
        {
            return Err(policy_error(
                "System package request is outside the approved Debian policy",
            ));
        }
        let package = need
            .package
            .as_deref()
            .ok_or_else(|| policy_error("System package name is missing"))?;
        let packages = crate::harness::resource_bridge::system_install_packages(
            &need.command,
            package,
            need.version.as_deref(),
        )
        .ok_or_else(|| {
            policy_error("System package request command is not a bounded apt install")
        })?;
        let manifest = need
            .introduced_packages
            .iter()
            .map(|package| {
                let architecture = package
                    .source
                    .rsplit('_')
                    .next()
                    .and_then(|part| part.strip_suffix(".deb"))
                    .unwrap_or_default()
                    .to_owned();
                crate::execution::docker::image_extension::builder::PackageArtifactIdentity {
                    name: package.package.clone(),
                    version: package.version.clone(),
                    architecture,
                    source: package.source.clone(),
                    sha256: package
                        .integrity
                        .strip_prefix("sha256:")
                        .unwrap_or_default()
                        .into(),
                }
            })
            .collect::<Vec<_>>();
        let proposal = self
            .system_proposal(&packages, &need.reason)
            .map_err(|error| {
                policy_error(&format!("System package proposal was rejected: {error:#}"))
            })?;
        let current = self
            .resolve_system_dependency_manifest(need)
            .map_err(|error| {
                policy_error(&format!(
                    "Signed package manifest could not be revalidated: {error:#}"
                ))
            })?;
        if current != need.introduced_packages {
            return Err(policy_error(
                "System package closure changed after the user reviewed it",
            ));
        }
        let authorized = proposal
            .authorize(decision, &self.workspace.task_id, manifest)
            .map_err(|error| {
                policy_error(&format!(
                    "System package authorization was rejected: {error:#}"
                ))
            })?;
        self.derive_authorized_bookworm_image(authorized)
    }

    /// Resolve the fixed signed Debian repository plan, require it to match the
    /// already authorized immutable manifest, then acquire and derive through
    /// the app-owned backend control plane. Worker commands cannot invoke this.
    pub(crate) fn derive_authorized_bookworm_image(
        &self,
        request: crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
    ) -> Result<crate::execution::docker::image_extension::DerivedImage, ExecutionError> {
        let architecture = request
            .target_platform
            .strip_prefix("linux/")
            .ok_or_else(|| policy_error("System image target platform is invalid"))?;
        let base = crate::execution::docker::image_extension::BaseImageSelection {
            reference: request.base_reference.clone(),
            digest: request.base_digest.clone(),
            profile: request.base_profile,
            platform: request.target_platform.clone(),
        };
        let installed_base = self
            .runtime
            .block_on(self.backend.inspect_system_package_state(&base))?;
        let plan = crate::harness::resource_bridge::RepositoryPackagePlan::resolve(
            &request
                .package_manifest
                .iter()
                .map(|package| package.name.clone())
                .collect::<Vec<_>>(),
            &installed_base,
            architecture,
        )
        .map_err(|error| {
            policy_error(&format!(
                "Unable to resolve approved Debian package closure: {error:#}"
            ))
        })?;
        if plan.identities() != request.package_manifest {
            return Err(policy_error(
                "Approved package manifest differs from the current signed Debian repository plan",
            ));
        }
        let packages = plan
            .fetch_authorized(&request.package_manifest)
            .map_err(|error| {
                policy_error(&format!(
                    "Unable to acquire verified Debian packages: {error:#}"
                ))
            })?;
        self.derive_authorized_system_image(request, packages)
    }

    pub(super) fn selected_image_digest(&self) -> Result<String, ExecutionError> {
        self.image_digest
            .read()
            .map(|digest| digest.clone())
            .map_err(|_| internal_error())
    }

    pub(crate) fn missing_locked_tools(
        &self,
        required_tools: &[String],
    ) -> Result<Vec<String>, ExecutionError> {
        let digest = self.selected_image_digest()?;
        let lock = crate::execution::docker::images::ImageLock::bundled().map_err(|error| {
            ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::ImageUnavailable,
                error.message,
                error.retryable,
            )
        })?;
        let image = lock
            .profiles
            .values()
            .flatten()
            .find(|image| image.digest == digest)
            .ok_or_else(|| {
                policy_error("Selected recipe base image is not present in the approved lock")
            })?;
        Ok(required_tools
            .iter()
            .filter(|tool| !image.tool_versions.contains_key(*tool))
            .cloned()
            .collect())
    }

    /// Ask the selected controller backend to derive and verify a system image
    /// from the explicitly authorized immutable closure, then pin this session
    /// to its content-addressed Docker image ID before the next command.
    pub(crate) fn derive_authorized_system_image(
        &self,
        request: crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
        packages: Vec<crate::execution::docker::image_extension::builder::VerifiedPackageArtifact>,
    ) -> Result<crate::execution::docker::image_extension::DerivedImage, ExecutionError> {
        self.verify_authorization()?;
        if self.busy.load(Ordering::Acquire)
            || request.task_id != self.workspace.task_id
            || request.project_id != self.workspace.project_id
            || request.base_digest != self.selected_image_digest()?
        {
            return Err(policy_error(
                "System image authorization is outside the idle task session scope",
            ));
        }
        let expected_manifest = request.package_manifest.clone();
        let result = self.runtime.block_on(
            self.backend
                .derive_authorized_system_image(&request, packages),
        )?;
        self.verify_authorization()?;
        let valid_digest = result
            .image_id
            .strip_prefix("sha256:")
            .is_some_and(|digest| {
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        if !valid_digest
            || result.base_digest != request.base_digest
            || result.target_platform != request.target_platform
            || result.package_manifest != expected_manifest
            || !result.content_identity.starts_with("sha256:")
        {
            return Err(policy_error(
                "Derived system image result does not match the authorized package closure",
            ));
        }
        *self.image_digest.write().map_err(|_| internal_error())? = result.image_id.clone();
        *self.derived_image.lock().map_err(|_| internal_error())? = Some(result.clone());
        Ok(result)
    }
}
