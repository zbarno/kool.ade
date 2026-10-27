use std::ffi::OsString;

use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

use super::TrustedArgument;

impl TrustedArgument {
    pub(in crate::app::bootstrap_remediation) fn as_os_string(&self) -> OsString {
        match self {
            Self::Flag(flag) => flag.as_str().into(),
            Self::Package(package) => package.as_str().into(),
            Self::VerifiedArtifact(artifact) => artifact.execution_path(),
            Self::DockerAptKeyDdInput(artifact)
            | Self::DockerAptSourcesDdInput(artifact)
            | Self::DockerDnfKeyDdInput(artifact)
            | Self::DockerDnfRepoDdInput(artifact) => {
                let mut value = OsString::from("if=");
                value.push(artifact.privileged_source_path());
                value
            }
            Self::LinuxRootlessRecipe(recipe) => match recipe {
                LinuxRootlessRecipeV1::Debian13Amd64 => "recipe:debian-13-amd64".into(),
                LinuxRootlessRecipeV1::Ubuntu2404Amd64 => "recipe:ubuntu-24.04-amd64".into(),
                LinuxRootlessRecipeV1::PopOs2404Amd64 => "recipe:pop-os-24.04-amd64".into(),
                LinuxRootlessRecipeV1::Fedora45Amd64 => "recipe:fedora-45-amd64".into(),
            },
            Self::AptAssumeYes => "--assume-yes".into(),
            Self::AptInstall => "install".into(),
            Self::AptGetVersion | Self::UsermodVersion => "--version".into(),
            Self::AptUpdate => "update".into(),
            Self::AptRootDirSystem => "--option=RootDir=/".into(),
            Self::AptSourceList => "--option=Dir::Etc::sourcelist=/etc/apt/sources.list".into(),
            Self::AptSourceParts => "--option=Dir::Etc::sourceparts=/etc/apt/sources.list.d".into(),
            Self::AptStateLists => "--option=Dir::State::lists=/var/lib/apt/lists".into(),
            Self::AptListCleanupTrue => "--option=APT::Get::List-Cleanup=true".into(),
            Self::AptArchitectureAmd64 => "--option=APT::Architecture=amd64".into(),
            Self::AptUnauthenticatedFalse => "--option=APT::Get::AllowUnauthenticated=false".into(),
            Self::AptInsecureRepositoriesFalse => {
                "--option=Acquire::AllowInsecureRepositories=false".into()
            }
            Self::AptWeakRepositoriesFalse => {
                "--option=Acquire::AllowWeakRepositories=false".into()
            }
            Self::AptDowngradeToInsecureFalse => {
                "--option=Acquire::AllowDowngradeToInsecureRepositories=false".into()
            }
            Self::AptCheckDateTrue => "--option=Acquire::Check-Date=true".into(),
            Self::AptCheckValidUntilTrue => "--option=Acquire::Check-Valid-Until=true".into(),
            Self::AptHttpsVerifyPeerTrue => "--option=Acquire::https::Verify-Peer=true".into(),
            Self::AptHttpsVerifyHostTrue => "--option=Acquire::https::Verify-Host=true".into(),
            Self::AptNoUpgrade => "--no-upgrade".into(),
            Self::AptNoRemove => "--no-remove".into(),
            Self::AptNoInstallRecommends => "--no-install-recommends".into(),
            Self::AptDockerCe => "docker-ce".into(),
            Self::AptDockerCeCli => "docker-ce-cli".into(),
            Self::AptContainerdIo => "containerd.io".into(),
            Self::AptDockerRootlessExtras => "docker-ce-rootless-extras".into(),
            Self::AptUidmap => "uidmap".into(),
            Self::AptDbusUserSession => "dbus-user-session".into(),
            Self::DdDockerAptKeyOutput => "of=/etc/apt/keyrings/docker.asc".into(),
            Self::DdDockerAptSourcesOutput => {
                "of=/etc/apt/sources.list.d/koolade-docker.sources".into()
            }
            Self::DdDockerDnfKeyOutput => "of=/etc/pki/rpm-gpg/RPM-GPG-KEY-koolade-docker".into(),
            Self::DdDockerDnfRepoOutput => "of=/etc/yum.repos.d/koolade-docker-ce.repo".into(),
            Self::DdOutputExclusive => "oflag=excl".into(),
            Self::DdSync => "conv=fsync".into(),
            Self::DdStatusNone => "status=none".into(),
            Self::MkdirMode0755 => "--mode=0755".into(),
            Self::MkdirDockerAptKeyringsDirectory => "/etc/apt/keyrings".into(),
            Self::DnfVersion => "--version".into(),
            Self::DnfQuiet => "--quiet".into(),
            Self::DnfCacheOnly => "--cacheonly".into(),
            Self::DnfReleaseVersion45 => "--releasever=45".into(),
            Self::DnfForceArchitectureX8664 => "--forcearch=x86_64".into(),
            Self::DnfRepositoryDirectory => {
                "--setopt=reposdir=/etc/yum.repos.d,/usr/share/dnf5/repos.d".into()
            }
            Self::DnfCachedir => "--setopt=cachedir=/var/cache/libdnf5".into(),
            Self::DnfSystemCachedir => "--setopt=system_cachedir=/var/cache/libdnf5".into(),
            Self::DnfDisablePlugins => "--setopt=plugins=0".into(),
            Self::DnfGpgCheck => "--setopt=gpgcheck=1".into(),
            Self::DnfPkgGpgCheckFalse => "--setopt=pkg_gpgcheck=0".into(),
            Self::DnfFedoraPkgGpgCheckFalse => "--setopt=fedora.pkg_gpgcheck=0".into(),
            Self::DnfUpdatesPkgGpgCheckFalse => "--setopt=updates.pkg_gpgcheck=0".into(),
            Self::DnfDockerPkgGpgCheckFalse => {
                "--setopt=koolade-docker-ce-stable.pkg_gpgcheck=0".into()
            }
            Self::DnfSslVerify => "--setopt=sslverify=1".into(),
            Self::DnfSslCaCertEmpty => "--setopt=sslcacert=".into(),
            Self::DnfProxySslVerify => "--setopt=proxy_sslverify=1".into(),
            Self::DnfProxySslCaCertEmpty => "--setopt=proxy_sslcacert=".into(),
            Self::DnfSkipIfUnavailableFalse => "--setopt=skip_if_unavailable=False".into(),
            Self::DnfFedoraGpgCheck => "--setopt=fedora.gpgcheck=1".into(),
            Self::DnfFedoraSslVerify => "--setopt=fedora.sslverify=1".into(),
            Self::DnfFedoraSslCaCertEmpty => "--setopt=fedora.sslcacert=".into(),
            Self::DnfFedoraProxySslVerify => "--setopt=fedora.proxy_sslverify=1".into(),
            Self::DnfFedoraProxySslCaCertEmpty => "--setopt=fedora.proxy_sslcacert=".into(),
            Self::DnfFedoraSkipIfUnavailableFalse => {
                "--setopt=fedora.skip_if_unavailable=False".into()
            }
            Self::DnfUpdatesGpgCheck => "--setopt=updates.gpgcheck=1".into(),
            Self::DnfUpdatesSslVerify => "--setopt=updates.sslverify=1".into(),
            Self::DnfUpdatesSslCaCertEmpty => "--setopt=updates.sslcacert=".into(),
            Self::DnfUpdatesProxySslVerify => "--setopt=updates.proxy_sslverify=1".into(),
            Self::DnfUpdatesProxySslCaCertEmpty => "--setopt=updates.proxy_sslcacert=".into(),
            Self::DnfUpdatesSkipIfUnavailableFalse => {
                "--setopt=updates.skip_if_unavailable=False".into()
            }
            Self::DnfDockerGpgCheck => "--setopt=koolade-docker-ce-stable.gpgcheck=1".into(),
            Self::DnfDockerSslVerify => "--setopt=koolade-docker-ce-stable.sslverify=1".into(),
            Self::DnfDockerSslCaCertEmpty => "--setopt=koolade-docker-ce-stable.sslcacert=".into(),
            Self::DnfDockerProxySslVerify => {
                "--setopt=koolade-docker-ce-stable.proxy_sslverify=1".into()
            }
            Self::DnfDockerProxySslCaCertEmpty => {
                "--setopt=koolade-docker-ce-stable.proxy_sslcacert=".into()
            }
            Self::DnfDockerSkipIfUnavailableFalse => {
                "--setopt=koolade-docker-ce-stable.skip_if_unavailable=False".into()
            }
            Self::DnfDisableAllRepositories => "--disablerepo=*".into(),
            Self::DnfEnableDockerRepository => "--enablerepo=koolade-docker-ce-stable".into(),
            Self::DnfEnableFedoraRepository => "--enablerepo=fedora".into(),
            Self::DnfEnableUpdatesRepository => "--enablerepo=updates".into(),
            Self::DnfMakeCache => "makecache".into(),
            Self::DnfRefresh => "--refresh".into(),
            Self::DnfInstall => "install".into(),
            Self::DnfAssumeYes => "--assumeyes".into(),
            Self::DnfBest => "--best".into(),
            Self::DnfNoWeakDependencies => "--setopt=install_weak_deps=False".into(),
            Self::DnfRepoQuery => "repoquery".into(),
            Self::DnfAvailable => "--available".into(),
            Self::DnfLatestLimitOne => "--latest-limit=1".into(),
            Self::DnfQueryFormatNameRepositoryId => "--qf=%{name}|%{repoid}".into(),
            Self::DnfDockerCe => "docker-ce".into(),
            Self::DnfDockerCeCli => "docker-ce-cli".into(),
            Self::DnfContainerdIo => "containerd.io".into(),
            Self::DnfDockerRootlessExtras => "docker-ce-rootless-extras".into(),
            Self::DnfShadowUtils => "shadow-utils".into(),
            Self::DnfSlirp4netns => "slirp4netns".into(),
            Self::DnfStageRoot(stage) => stage.root_path().into_os_string(),
            Self::DnfStagePackages(stage) | Self::FindStagePackages(stage) => {
                stage.packages_path().into_os_string()
            }
            Self::DnfStageDatabase(stage) => stage.rpmdb_path().into_os_string(),
            Self::DnfStageCacheDirectory(stage) => stage.cache_path().into_os_string(),
            Self::DnfStagedRpm(package) => package.path().into_os_string(),
            Self::DnfHashBoundRpm(package) => package.path().into_os_string(),
            Self::DnfVerifiedRpm(package) => package.path().into_os_string(),
            Self::DnfDestDir(stage) => {
                let mut value = OsString::from("--setopt=destdir=");
                value.push(stage.packages_path());
                value
            }
            Self::DnfStageCachedir(stage) => {
                let mut value = OsString::from("--setopt=cachedir=");
                value.push(stage.cache_path());
                value
            }
            Self::DnfStageSystemCachedir(stage) => {
                let mut value = OsString::from("--setopt=system_cachedir=");
                value.push(stage.cache_path());
                value
            }
            Self::DnfDownloadOnly => "--downloadonly".into(),
            Self::DnfLocalPackageGpgCheckFalse => "--setopt=localpkg_gpgcheck=0".into(),
            Self::DnfInstallOnlyPackages(names) => {
                let mut value = OsString::from("--setopt=installonlypkgs=");
                value.push(names.csv());
                value
            }
            Self::DnfInstallOnlyLimitUnlimited => "--setopt=installonly_limit=0".into(),
            Self::DnfObsoletesFalse => "--setopt=obsoletes=False".into(),
            Self::DnfNoAllowDowngrade => "--no-allow-downgrade".into(),
            Self::RpmDbPath(stage) => {
                let mut value = OsString::from("--dbpath=");
                value.push(stage.rpmdb_path());
                value
            }
            Self::RpmDefineMacro => "--define".into(),
            Self::RpmKeyringRpmdb => "_keyring rpmdb".into(),
            Self::RpmVerifyLevelAll => "_pkgverify_level all".into(),
            Self::RpmVerifyFlagsZero => "_pkgverify_flags 0x0".into(),
            Self::RpmInitDb => "--initdb".into(),
            Self::RpmImportKeys => "--import".into(),
            Self::RpmListKeys => "--list".into(),
            Self::RpmCheckSignature => "--checksig".into(),
            Self::RpmVerbose => "--verbose".into(),
            Self::RpmDockerKey => "/etc/pki/rpm-gpg/RPM-GPG-KEY-koolade-docker".into(),
            Self::RpmFedoraKey => "/usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-45-x86_64".into(),
            Self::RpmQuery => "--query".into(),
            Self::RpmQueryPackage => "--package".into(),
            Self::RpmQueryAll => "--all".into(),
            Self::RpmQueryFormatName => "--queryformat=%{NAME}\\n".into(),
            Self::Sha256sumBinary => "--binary".into(),
            Self::FindMindepth => "-mindepth".into(),
            Self::FindMaxdepth => "-maxdepth".into(),
            Self::FindOne => "1".into(),
            Self::FindPrintf => "-printf".into(),
            Self::FindRpmMetadataFormat => "%f\\0%y\\0%U\\0%G\\0%m\\0%s\\0%n\\0".into(),
            Self::MkdirMode0700 => "--mode=0700".into(),
            Self::RmRecursive => "--recursive".into(),
            Self::RmForce => "--force".into(),
            Self::EndOfOptions => "--".into(),
            Self::UsermodAddSubuids => "--add-subuids".into(),
            Self::UsermodSubuidRange { start, end } | Self::UsermodSubgidRange { start, end } => {
                format!("{start}-{end}").into()
            }
            Self::UsermodAddSubgids => "--add-subgids".into(),
            Self::UsermodEndOfOptions => "--".into(),
            Self::CurrentUserName { name, .. } => name.into(),
            Self::SystemctlUser => "--user".into(),
            Self::SystemctlShow => "show".into(),
            Self::SystemctlDaemonReload => "daemon-reload".into(),
            Self::SystemctlIsActive => "is-active".into(),
            Self::SystemctlEnable => "enable".into(),
            Self::SystemctlNow => "--now".into(),
            Self::SystemctlPropertyLoadState => "--property=LoadState".into(),
            Self::SystemctlPropertyActiveState => "--property=ActiveState".into(),
            Self::SystemctlPropertyUnitFileState => "--property=UnitFileState".into(),
            Self::SystemctlValue => "--value".into(),
            Self::LinuxRootlessServiceUnit => "koolade-rootless-docker.service".into(),
            Self::RootlesskitTrueExecutable => "/usr/bin/true".into(),
            #[cfg(test)]
            Self::TestLiteral(value) => value.into(),
        }
    }
}
