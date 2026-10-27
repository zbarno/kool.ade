use super::super::{DnfStageId, TrustedArgument};

pub(super) fn validate(argument: &TrustedArgument) -> anyhow::Result<()> {
    match argument {
        TrustedArgument::DnfStageRoot(stage)
        | TrustedArgument::DnfStagePackages(stage)
        | TrustedArgument::DnfStageDatabase(stage)
        | TrustedArgument::DnfStageCacheDirectory(stage)
        | TrustedArgument::DnfDestDir(stage)
        | TrustedArgument::DnfStageCachedir(stage)
        | TrustedArgument::DnfStageSystemCachedir(stage)
        | TrustedArgument::RpmDbPath(stage)
        | TrustedArgument::FindStagePackages(stage) => stage.validate(),
        TrustedArgument::DnfStagedRpm(package) => package.validate(),
        TrustedArgument::DnfHashBoundRpm(package) => package.validate(),
        TrustedArgument::DnfVerifiedRpm(package) => package.validate(),
        TrustedArgument::DnfInstallOnlyPackages(names) => names.validate(),
        _ => Ok(()),
    }
}

pub(super) fn binding_value(argument: &TrustedArgument) -> String {
    match argument {
        TrustedArgument::DnfVersion => "fixed:dnf-version".into(),
        TrustedArgument::DnfQuiet => "fixed:dnf-quiet".into(),
        TrustedArgument::DnfCacheOnly => "fixed:dnf-cache-only".into(),
        TrustedArgument::DnfReleaseVersion45 => "fixed:dnf-release-version-45".into(),
        TrustedArgument::DnfForceArchitectureX8664 => "fixed:dnf-force-architecture-x86_64".into(),
        TrustedArgument::DnfRepositoryDirectory => "fixed:dnf-repository-directory".into(),
        TrustedArgument::DnfCachedir => "fixed:dnf-cachedir".into(),
        TrustedArgument::DnfSystemCachedir => "fixed:dnf-system-cachedir".into(),
        TrustedArgument::DnfDisablePlugins => "fixed:dnf-disable-plugins".into(),
        TrustedArgument::DnfGpgCheck => "fixed:dnf-gpg-check".into(),
        TrustedArgument::DnfPkgGpgCheckFalse => "fixed:dnf-pkg-gpg-check-false".into(),
        TrustedArgument::DnfFedoraPkgGpgCheckFalse => "fixed:dnf-fedora-pkg-gpg-check-false".into(),
        TrustedArgument::DnfUpdatesPkgGpgCheckFalse => {
            "fixed:dnf-updates-pkg-gpg-check-false".into()
        }
        TrustedArgument::DnfDockerPkgGpgCheckFalse => "fixed:dnf-docker-pkg-gpg-check-false".into(),
        TrustedArgument::DnfSslVerify => "fixed:dnf-ssl-verify".into(),
        TrustedArgument::DnfSslCaCertEmpty => "fixed:dnf-ssl-cacert-empty".into(),
        TrustedArgument::DnfProxySslVerify => "fixed:dnf-proxy-ssl-verify".into(),
        TrustedArgument::DnfProxySslCaCertEmpty => "fixed:dnf-proxy-ssl-cacert-empty".into(),
        TrustedArgument::DnfSkipIfUnavailableFalse => "fixed:dnf-skip-if-unavailable-false".into(),
        TrustedArgument::DnfFedoraGpgCheck => "fixed:dnf-fedora-gpg-check".into(),
        TrustedArgument::DnfFedoraSslVerify => "fixed:dnf-fedora-ssl-verify".into(),
        TrustedArgument::DnfFedoraSslCaCertEmpty => "fixed:dnf-fedora-ssl-cacert-empty".into(),
        TrustedArgument::DnfFedoraProxySslVerify => "fixed:dnf-fedora-proxy-ssl-verify".into(),
        TrustedArgument::DnfFedoraProxySslCaCertEmpty => {
            "fixed:dnf-fedora-proxy-ssl-cacert-empty".into()
        }
        TrustedArgument::DnfFedoraSkipIfUnavailableFalse => {
            "fixed:dnf-fedora-skip-if-unavailable-false".into()
        }
        TrustedArgument::DnfUpdatesGpgCheck => "fixed:dnf-updates-gpg-check".into(),
        TrustedArgument::DnfUpdatesSslVerify => "fixed:dnf-updates-ssl-verify".into(),
        TrustedArgument::DnfUpdatesSslCaCertEmpty => "fixed:dnf-updates-ssl-cacert-empty".into(),
        TrustedArgument::DnfUpdatesProxySslVerify => "fixed:dnf-updates-proxy-ssl-verify".into(),
        TrustedArgument::DnfUpdatesProxySslCaCertEmpty => {
            "fixed:dnf-updates-proxy-ssl-cacert-empty".into()
        }
        TrustedArgument::DnfUpdatesSkipIfUnavailableFalse => {
            "fixed:dnf-updates-skip-if-unavailable-false".into()
        }
        TrustedArgument::DnfDockerGpgCheck => "fixed:dnf-docker-gpg-check".into(),
        TrustedArgument::DnfDockerSslVerify => "fixed:dnf-docker-ssl-verify".into(),
        TrustedArgument::DnfDockerSslCaCertEmpty => "fixed:dnf-docker-ssl-cacert-empty".into(),
        TrustedArgument::DnfDockerProxySslVerify => "fixed:dnf-docker-proxy-ssl-verify".into(),
        TrustedArgument::DnfDockerProxySslCaCertEmpty => {
            "fixed:dnf-docker-proxy-ssl-cacert-empty".into()
        }
        TrustedArgument::DnfDockerSkipIfUnavailableFalse => {
            "fixed:dnf-docker-skip-if-unavailable-false".into()
        }
        TrustedArgument::DnfDisableAllRepositories => "fixed:dnf-disable-all-repositories".into(),
        TrustedArgument::DnfEnableDockerRepository => "fixed:dnf-enable-docker-repository".into(),
        TrustedArgument::DnfEnableFedoraRepository => "fixed:dnf-enable-fedora-repository".into(),
        TrustedArgument::DnfEnableUpdatesRepository => "fixed:dnf-enable-updates-repository".into(),
        TrustedArgument::DnfMakeCache => "fixed:dnf-make-cache".into(),
        TrustedArgument::DnfRefresh => "fixed:dnf-refresh".into(),
        TrustedArgument::DnfInstall => "fixed:dnf-install".into(),
        TrustedArgument::DnfAssumeYes => "fixed:dnf-assume-yes".into(),
        TrustedArgument::DnfBest => "fixed:dnf-best".into(),
        TrustedArgument::DnfNoWeakDependencies => "fixed:dnf-no-weak-dependencies".into(),
        TrustedArgument::DnfRepoQuery => "fixed:dnf-repoquery".into(),
        TrustedArgument::DnfAvailable => "fixed:dnf-available".into(),
        TrustedArgument::DnfLatestLimitOne => "fixed:dnf-latest-limit-one".into(),
        TrustedArgument::DnfQueryFormatNameRepositoryId => {
            "fixed:dnf-query-format-name-repository-id".into()
        }
        TrustedArgument::DnfDockerCe => "fixed:dnf-docker-ce".into(),
        TrustedArgument::DnfDockerCeCli => "fixed:dnf-docker-ce-cli".into(),
        TrustedArgument::DnfContainerdIo => "fixed:dnf-containerd-io".into(),
        TrustedArgument::DnfDockerRootlessExtras => "fixed:dnf-docker-ce-rootless-extras".into(),
        TrustedArgument::DnfShadowUtils => "fixed:dnf-shadow-utils".into(),
        TrustedArgument::DnfSlirp4netns => "fixed:dnf-slirp4netns".into(),
        TrustedArgument::DnfStageRoot(stage) => stage_value("dnf-stage-root", stage),
        TrustedArgument::DnfStagePackages(stage) => stage_value("dnf-stage-packages", stage),
        TrustedArgument::DnfStageDatabase(stage) => stage_value("dnf-stage-database", stage),
        TrustedArgument::DnfStageCacheDirectory(stage) => stage_value("dnf-stage-cache", stage),
        TrustedArgument::DnfStagedRpm(package) => {
            format!("dnf-staged-rpm:{}", package.binding_value())
        }
        TrustedArgument::DnfHashBoundRpm(package) => {
            format!("dnf-hashbound-rpm:{}", package.binding_value())
        }
        TrustedArgument::DnfVerifiedRpm(package) => {
            format!("dnf-verified-rpm:{}", package.binding_value())
        }
        TrustedArgument::DnfDestDir(stage) => stage_value("dnf-destdir", stage),
        TrustedArgument::DnfStageCachedir(stage) => stage_value("dnf-cachedir", stage),
        TrustedArgument::DnfStageSystemCachedir(stage) => stage_value("dnf-system-cachedir", stage),
        TrustedArgument::DnfDownloadOnly => "fixed:dnf-downloadonly".into(),
        TrustedArgument::DnfLocalPackageGpgCheckFalse => "fixed:dnf-localpkg-gpgcheck-false".into(),
        TrustedArgument::DnfInstallOnlyPackages(names) => {
            format!(
                "dnf-installonly-packages:{}:{}",
                names.stage().binding_id(),
                names.csv()
            )
        }
        TrustedArgument::DnfInstallOnlyLimitUnlimited => {
            "fixed:dnf-installonly-limit-unlimited".into()
        }
        TrustedArgument::DnfObsoletesFalse => "fixed:dnf-obsoletes-false".into(),
        TrustedArgument::DnfNoAllowDowngrade => "fixed:dnf-no-allow-downgrade".into(),
        TrustedArgument::RpmDbPath(stage) => stage_value("rpm-dbpath", stage),
        TrustedArgument::RpmDefineMacro => "fixed:rpm-define".into(),
        TrustedArgument::RpmKeyringRpmdb => "fixed:rpm-keyring-rpmdb".into(),
        TrustedArgument::RpmVerifyLevelAll => "fixed:rpm-verify-level-all".into(),
        TrustedArgument::RpmVerifyFlagsZero => "fixed:rpm-verify-flags-zero".into(),
        TrustedArgument::RpmInitDb => "fixed:rpm-initdb".into(),
        TrustedArgument::RpmImportKeys => "fixed:rpm-import-keys".into(),
        TrustedArgument::RpmListKeys => "fixed:rpm-list-keys".into(),
        TrustedArgument::RpmCheckSignature => "fixed:rpm-checksig".into(),
        TrustedArgument::RpmVerbose => "fixed:rpm-verbose".into(),
        TrustedArgument::RpmDockerKey => "fixed:rpm-docker-key".into(),
        TrustedArgument::RpmFedoraKey => "fixed:rpm-fedora-key".into(),
        TrustedArgument::RpmQuery => "fixed:rpm-query".into(),
        TrustedArgument::RpmQueryPackage => "fixed:rpm-query-package".into(),
        TrustedArgument::RpmQueryAll => "fixed:rpm-query-all".into(),
        TrustedArgument::RpmQueryFormatName => "fixed:rpm-query-format-name".into(),
        TrustedArgument::Sha256sumBinary => "fixed:sha256sum-binary".into(),
        TrustedArgument::FindStagePackages(stage) => stage_value("find-stage-packages", stage),
        TrustedArgument::FindMindepth => "fixed:find-mindepth".into(),
        TrustedArgument::FindMaxdepth => "fixed:find-maxdepth".into(),
        TrustedArgument::FindOne => "fixed:find-one".into(),
        TrustedArgument::FindPrintf => "fixed:find-printf".into(),
        TrustedArgument::FindRpmMetadataFormat => "fixed:find-rpm-metadata-format".into(),
        TrustedArgument::MkdirMode0700 => "fixed:mkdir-mode-0700".into(),
        TrustedArgument::RmRecursive => "fixed:rm-recursive".into(),
        TrustedArgument::RmForce => "fixed:rm-force".into(),
        TrustedArgument::EndOfOptions => "fixed:end-of-options".into(),
        _ => unreachable!("only rootless DNF arguments are handled here"),
    }
}

fn stage_value(prefix: &str, stage: &DnfStageId) -> String {
    format!("{prefix}:{}", stage.binding_id())
}
