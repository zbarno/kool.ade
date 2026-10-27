use super::super::super::catalog::TrustedFlag;
use super::TrustedArgument;

mod current_user;
mod dnf;

impl TrustedArgument {
    pub(in crate::app::bootstrap_remediation::supervisor::process) fn validate(
        &self,
    ) -> anyhow::Result<()> {
        if let Self::VerifiedArtifact(artifact) = self {
            artifact.validate()?;
        }
        match self {
            Self::DockerAptKeyDdInput(artifact) => {
                artifact.validate()?;
                anyhow::ensure!(
                    artifact.digest()
                        == crate::app::bootstrap_remediation::trusted_assets::DOCKER_APT_KEY_SHA256,
                    "repository key artifact differs from the pinned Docker key"
                );
            }
            Self::DockerAptSourcesDdInput(artifact) => {
                artifact.validate()?;
                anyhow::ensure!(
                    artifact.digest() == crate::app::bootstrap_remediation::trusted_assets::DOCKER_APT_SOURCES_SHA256,
                    "repository source artifact differs from the pinned Docker source"
                );
            }
            Self::DockerDnfKeyDdInput(artifact) => {
                artifact.validate()?;
                anyhow::ensure!(
                    artifact.digest()
                        == crate::app::bootstrap_remediation::trusted_assets::DOCKER_DNF_KEY_SHA256,
                    "repository key artifact differs from the pinned Docker Fedora key"
                );
            }
            Self::DockerDnfRepoDdInput(artifact) => {
                artifact.validate()?;
                anyhow::ensure!(
                    artifact.digest()
                        == crate::app::bootstrap_remediation::trusted_assets::DOCKER_DNF_REPO_SHA256,
                    "repository source artifact differs from the pinned Docker Fedora source"
                );
            }
            Self::LinuxRootlessRecipe(_) => {}
            Self::UsermodSubuidRange { start, end } | Self::UsermodSubgidRange { start, end } => {
                anyhow::ensure!(
                    *start == 231_072 && *end == 296_607,
                    "rootless subordinate ID range differs from the approved recipe"
                );
            }
            Self::CurrentUserName { uid, name } => current_user::validate_name(*uid, name)?,
            _ => dnf::validate(self)?,
        }
        Ok(())
    }

    pub(in crate::app::bootstrap_remediation::supervisor::process) fn binding_value(
        &self,
    ) -> String {
        match self {
            Self::Flag(flag) => format!("flag:{flag:?}"),
            Self::Package(package) => format!("package:{package:?}"),
            Self::VerifiedArtifact(artifact) => {
                format!("artifact:{}:{}", artifact.path.display(), artifact.sha256)
            }
            Self::DockerAptKeyDdInput(artifact) => {
                format!("docker-apt-key:{}", artifact.digest())
            }
            Self::DockerAptSourcesDdInput(artifact) => {
                format!("docker-apt-sources:{}", artifact.digest())
            }
            Self::DockerDnfKeyDdInput(artifact) => {
                format!("docker-dnf-key:{}", artifact.digest())
            }
            Self::DockerDnfRepoDdInput(artifact) => {
                format!("docker-dnf-repo:{}", artifact.digest())
            }
            Self::LinuxRootlessRecipe(recipe) => format!("rootless-recipe:{recipe:?}"),
            Self::CurrentUserName { uid, name } => format!("current-user:{uid}:{name}"),
            Self::SystemctlUser => "fixed:systemctl-user".into(),
            Self::SystemctlShow => "fixed:systemctl-show".into(),
            Self::SystemctlDaemonReload => "fixed:systemctl-daemon-reload".into(),
            Self::SystemctlIsActive => "fixed:systemctl-is-active".into(),
            Self::SystemctlEnable => "fixed:systemctl-enable".into(),
            Self::SystemctlNow => "fixed:systemctl-now".into(),
            Self::SystemctlPropertyLoadState => "fixed:systemctl-property-load-state".into(),
            Self::SystemctlPropertyActiveState => "fixed:systemctl-property-active-state".into(),
            Self::SystemctlPropertyUnitFileState => {
                "fixed:systemctl-property-unit-file-state".into()
            }
            Self::SystemctlValue => "fixed:systemctl-value".into(),
            Self::LinuxRootlessServiceUnit => "fixed:linux-rootless-service-unit".into(),
            Self::RootlesskitTrueExecutable => "fixed:rootlesskit-true-executable".into(),
            Self::AptAssumeYes => "fixed:apt-assume-yes".into(),
            Self::AptInstall => "fixed:apt-install".into(),
            Self::AptGetVersion => "fixed:apt-version".into(),
            Self::AptUpdate => "fixed:apt-update".into(),
            Self::AptRootDirSystem => "fixed:apt-root-dir-system".into(),
            Self::AptSourceList => "fixed:apt-source-list".into(),
            Self::AptSourceParts => "fixed:apt-source-parts".into(),
            Self::AptStateLists => "fixed:apt-state-lists".into(),
            Self::AptListCleanupTrue => "fixed:apt-list-cleanup-true".into(),
            Self::AptArchitectureAmd64 => "fixed:apt-architecture-amd64".into(),
            Self::AptUnauthenticatedFalse => "fixed:apt-unauthenticated-false".into(),
            Self::AptInsecureRepositoriesFalse => "fixed:apt-insecure-repositories-false".into(),
            Self::AptWeakRepositoriesFalse => "fixed:apt-weak-repositories-false".into(),
            Self::AptDowngradeToInsecureFalse => "fixed:apt-downgrade-to-insecure-false".into(),
            Self::AptCheckDateTrue => "fixed:apt-check-date-true".into(),
            Self::AptCheckValidUntilTrue => "fixed:apt-check-valid-until-true".into(),
            Self::AptHttpsVerifyPeerTrue => "fixed:apt-https-verify-peer-true".into(),
            Self::AptHttpsVerifyHostTrue => "fixed:apt-https-verify-host-true".into(),
            Self::AptNoUpgrade => "fixed:apt-no-upgrade".into(),
            Self::AptNoRemove => "fixed:apt-no-remove".into(),
            Self::AptNoInstallRecommends => "fixed:apt-no-install-recommends".into(),
            Self::AptDockerCe => "fixed:apt-docker-ce".into(),
            Self::AptDockerCeCli => "fixed:apt-docker-ce-cli".into(),
            Self::AptContainerdIo => "fixed:apt-containerd-io".into(),
            Self::AptDockerRootlessExtras => "fixed:apt-docker-rootless-extras".into(),
            Self::AptUidmap => "fixed:apt-uidmap".into(),
            Self::AptDbusUserSession => "fixed:apt-dbus-user-session".into(),
            Self::DdDockerAptKeyOutput => "fixed:dd-docker-apt-key-output".into(),
            Self::DdDockerAptSourcesOutput => "fixed:dd-docker-apt-sources-output".into(),
            Self::DdDockerDnfKeyOutput => "fixed:dd-docker-dnf-key-output".into(),
            Self::DdDockerDnfRepoOutput => "fixed:dd-docker-dnf-repo-output".into(),
            Self::DdOutputExclusive => "fixed:dd-output-exclusive".into(),
            Self::DdSync => "fixed:dd-sync".into(),
            Self::DdStatusNone => "fixed:dd-status-none".into(),
            Self::MkdirMode0755 => "fixed:mkdir-mode-0755".into(),
            Self::MkdirDockerAptKeyringsDirectory => "fixed:docker-apt-keyrings-directory".into(),
            argument @ (Self::DnfVersion
            | Self::DnfQuiet
            | Self::DnfCacheOnly
            | Self::DnfReleaseVersion45
            | Self::DnfForceArchitectureX8664
            | Self::DnfRepositoryDirectory
            | Self::DnfCachedir
            | Self::DnfSystemCachedir
            | Self::DnfDisablePlugins
            | Self::DnfGpgCheck
            | Self::DnfPkgGpgCheckFalse
            | Self::DnfFedoraPkgGpgCheckFalse
            | Self::DnfUpdatesPkgGpgCheckFalse
            | Self::DnfDockerPkgGpgCheckFalse
            | Self::DnfSslVerify
            | Self::DnfSslCaCertEmpty
            | Self::DnfProxySslVerify
            | Self::DnfProxySslCaCertEmpty
            | Self::DnfSkipIfUnavailableFalse
            | Self::DnfFedoraGpgCheck
            | Self::DnfFedoraSslVerify
            | Self::DnfFedoraSslCaCertEmpty
            | Self::DnfFedoraProxySslVerify
            | Self::DnfFedoraProxySslCaCertEmpty
            | Self::DnfFedoraSkipIfUnavailableFalse
            | Self::DnfUpdatesGpgCheck
            | Self::DnfUpdatesSslVerify
            | Self::DnfUpdatesSslCaCertEmpty
            | Self::DnfUpdatesProxySslVerify
            | Self::DnfUpdatesProxySslCaCertEmpty
            | Self::DnfUpdatesSkipIfUnavailableFalse
            | Self::DnfDockerGpgCheck
            | Self::DnfDockerSslVerify
            | Self::DnfDockerSslCaCertEmpty
            | Self::DnfDockerProxySslVerify
            | Self::DnfDockerProxySslCaCertEmpty
            | Self::DnfDockerSkipIfUnavailableFalse
            | Self::DnfDisableAllRepositories
            | Self::DnfEnableDockerRepository
            | Self::DnfEnableFedoraRepository
            | Self::DnfEnableUpdatesRepository
            | Self::DnfMakeCache
            | Self::DnfRefresh
            | Self::DnfInstall
            | Self::DnfAssumeYes
            | Self::DnfBest
            | Self::DnfNoWeakDependencies
            | Self::DnfRepoQuery
            | Self::DnfAvailable
            | Self::DnfLatestLimitOne
            | Self::DnfQueryFormatNameRepositoryId
            | Self::DnfDockerCe
            | Self::DnfDockerCeCli
            | Self::DnfContainerdIo
            | Self::DnfDockerRootlessExtras
            | Self::DnfShadowUtils
            | Self::DnfSlirp4netns
            | Self::DnfStageRoot(_)
            | Self::DnfStagePackages(_)
            | Self::DnfStageDatabase(_)
            | Self::DnfStageCacheDirectory(_)
            | Self::DnfStagedRpm(_)
            | Self::DnfHashBoundRpm(_)
            | Self::DnfVerifiedRpm(_)
            | Self::DnfDestDir(_)
            | Self::DnfStageCachedir(_)
            | Self::DnfStageSystemCachedir(_)
            | Self::DnfDownloadOnly
            | Self::DnfLocalPackageGpgCheckFalse
            | Self::DnfInstallOnlyPackages(_)
            | Self::DnfInstallOnlyLimitUnlimited
            | Self::DnfObsoletesFalse
            | Self::DnfNoAllowDowngrade
            | Self::RpmDbPath(_)
            | Self::RpmDefineMacro
            | Self::RpmKeyringRpmdb
            | Self::RpmVerifyLevelAll
            | Self::RpmVerifyFlagsZero
            | Self::RpmInitDb
            | Self::RpmImportKeys
            | Self::RpmListKeys
            | Self::RpmCheckSignature
            | Self::RpmVerbose
            | Self::RpmDockerKey
            | Self::RpmFedoraKey
            | Self::RpmQuery
            | Self::RpmQueryPackage
            | Self::RpmQueryAll
            | Self::RpmQueryFormatName
            | Self::Sha256sumBinary
            | Self::FindStagePackages(_)
            | Self::FindMindepth
            | Self::FindMaxdepth
            | Self::FindOne
            | Self::FindPrintf
            | Self::FindRpmMetadataFormat
            | Self::MkdirMode0700
            | Self::RmRecursive
            | Self::RmForce
            | Self::EndOfOptions) => dnf::binding_value(argument),
            Self::UsermodAddSubuids => "fixed:usermod-add-subuids".into(),
            Self::UsermodVersion => "fixed:usermod-version".into(),
            Self::UsermodSubuidRange { start, end } => {
                format!("fixed:usermod-subuid-range:{start}-{end}")
            }
            Self::UsermodAddSubgids => "fixed:usermod-add-subgids".into(),
            Self::UsermodSubgidRange { start, end } => {
                format!("fixed:usermod-subgid-range:{start}-{end}")
            }
            Self::UsermodEndOfOptions => "fixed:usermod-end-options".into(),
            #[cfg(test)]
            Self::TestLiteral(value) => format!("test:{value}"),
        }
    }
}

impl TrustedFlag {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::AptInstall => "install",
            Self::AssumeYes => "--assume-yes",
            Self::Quiet => "--quiet",
            Self::MsiInstall => "/i",
            Self::NoRestart => "/norestart",
        }
    }
}
