use sha2::{Digest, Sha256};
use std::{sync::mpsc::Sender, time::Duration};

mod arguments;
mod dnf;
pub(crate) use arguments::{
    DNF_MAX_STAGED_RPMS, DnfPackageNames, DnfStageId, HashBoundDnfRpm, StagedRpm, TrustedArgument,
    VerifiedDnfRpm, parse_installed_package_names,
};

use super::catalog::TrustedProgram;
use crate::app::bootstrap_remediation::claude_code_install::ClaudeInstallProgress;
use crate::app::bootstrap_repair_policy::Host;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

const MAX_RUNTIME: Duration = Duration::from_secs(600);
const MAX_CLAUDE_RUNTIME: Duration = Duration::from_secs(20 * 60);

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClaudeReleaseAsset {
    Manifest,
    Signature,
    PublicKey,
    LinuxX64Binary,
}

#[cfg(target_os = "linux")]
impl ClaudeReleaseAsset {
    pub(crate) fn url(self) -> &'static str {
        use crate::app::bootstrap_remediation::claude_code_install::recipe;
        match self {
            Self::Manifest => recipe::MANIFEST_URL,
            Self::Signature => recipe::SIGNATURE_URL,
            Self::PublicKey => recipe::KEY_URL,
            Self::LinuxX64Binary => recipe::BINARY_URL,
        }
    }
}

#[derive(Debug)]
pub(crate) enum InternalInvocation {
    ClaudeCodeInstall {
        target: std::path::PathBuf,
    },
    #[cfg(target_os = "linux")]
    ClaudeCodeDownload {
        asset: ClaudeReleaseAsset,
        destination: std::path::PathBuf,
        max_bytes: u64,
    },
}

pub(crate) struct TrustedInvocation {
    pub(in crate::app::bootstrap_remediation::supervisor::process) host: Host,
    pub(in crate::app::bootstrap_remediation::supervisor::process) program: TrustedProgram,
    pub(in crate::app::bootstrap_remediation::supervisor::process) arguments: Vec<TrustedArgument>,
    pub(in crate::app::bootstrap_remediation::supervisor::process) timeout: Duration,
    pub(in crate::app::bootstrap_remediation::supervisor::process) internal:
        Option<InternalInvocation>,
    pub(in crate::app::bootstrap_remediation::supervisor::process) progress:
        Option<Sender<ClaudeInstallProgress>>,
    pub(in crate::app::bootstrap_remediation::supervisor::process) environment: TrustedEnvironment,
    #[cfg(test)]
    pub(in crate::app::bootstrap_remediation::supervisor::process) test_mode: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrustedEnvironment {
    Isolated,
    CurrentUserSystemd,
}

impl TrustedInvocation {
    pub(crate) fn host(&self) -> Host {
        self.host
    }

    pub(crate) fn program(&self) -> TrustedProgram {
        self.program
    }

    pub(crate) fn current_user_identity(&self) -> anyhow::Result<(u32, &str)> {
        anyhow::ensure!(
            self.program == TrustedProgram::Usermod,
            "current account binding is only valid for usermod"
        );
        let mut identities = self.arguments.iter().filter_map(|argument| match argument {
            TrustedArgument::CurrentUserName { uid, name } => Some((*uid, name.as_str())),
            _ => None,
        });
        let identity = identities
            .next()
            .ok_or_else(|| anyhow::anyhow!("usermod invocation lacks a bound current account"))?;
        anyhow::ensure!(
            identities.next().is_none(),
            "usermod invocation has multiple account identities"
        );
        Ok(identity)
    }

    pub(crate) fn rootless_package_recipe(&self) -> anyhow::Result<LinuxRootlessRecipeV1> {
        anyhow::ensure!(
            self.program == TrustedProgram::LinuxRootlessPackageSetup,
            "rootless recipe binding is only valid for package setup"
        );
        let mut recipes = self.arguments.iter().filter_map(|argument| match argument {
            TrustedArgument::LinuxRootlessRecipe(recipe) => Some(*recipe),
            _ => None,
        });
        let recipe = recipes
            .next()
            .ok_or_else(|| anyhow::anyhow!("rootless package setup lacks a recipe binding"))?;
        anyhow::ensure!(
            recipes.next().is_none(),
            "rootless package setup has multiple recipe bindings"
        );
        Ok(recipe)
    }

    #[cfg(unix)]
    pub(in crate::app::bootstrap_remediation::supervisor::process) fn artifact_fds(
        &self,
    ) -> Vec<std::os::fd::RawFd> {
        use std::os::fd::AsRawFd;

        self.arguments
            .iter()
            .filter_map(|argument| match argument {
                TrustedArgument::VerifiedArtifact(artifact) => Some(artifact.file.as_raw_fd()),
                _ => None,
            })
            .collect()
    }

    pub(in crate::app::bootstrap_remediation) fn new(
        host: Host,
        program: TrustedProgram,
        arguments: Vec<TrustedArgument>,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            timeout > Duration::ZERO && timeout <= MAX_RUNTIME,
            "remediation timeout is outside policy"
        );
        let bounded_variable_arguments = match program {
            TrustedProgram::Dnf => dnf::trusted_arguments(&arguments),
            TrustedProgram::RpmKeys => dnf::trusted_rpmkeys_arguments(&arguments),
            TrustedProgram::Rpm => dnf::trusted_rpm_query_arguments(&arguments),
            TrustedProgram::Sha256sum => dnf::trusted_sha256sum_arguments(&arguments),
            _ => false,
        };
        anyhow::ensure!(
            arguments.len() <= if bounded_variable_arguments { 256 } else { 48 },
            "too many remediation arguments"
        );
        anyhow::ensure!(
            program.matches(host),
            "trusted program does not match the planned host"
        );
        anyhow::ensure!(
            !matches!(
                program,
                TrustedProgram::ClaudeCodeInstall | TrustedProgram::ClaudeCodeDownload
            ),
            "internal remediation programs require a typed constructor"
        );
        if program == TrustedProgram::Dd {
            anyhow::ensure!(
                trusted_docker_repository_create(&arguments),
                "dd is restricted to exclusive creation of the pinned Docker repository files"
            );
        }
        if program == TrustedProgram::Dnf {
            anyhow::ensure!(
                dnf::trusted_arguments(&arguments),
                "DNF is restricted to approved Fedora 45 rootless package operations"
            );
        }
        if program == TrustedProgram::Mkdir {
            anyhow::ensure!(
                matches!(
                    arguments.as_slice(),
                    [
                        TrustedArgument::MkdirMode0755,
                        TrustedArgument::MkdirDockerAptKeyringsDirectory
                    ]
                ) || dnf::trusted_stage_mkdir_arguments(&arguments),
                "mkdir is restricted to approved keyring or private DNF stage directories"
            );
        }
        if program == TrustedProgram::Find {
            anyhow::ensure!(
                dnf::trusted_find_arguments(&arguments),
                "find is restricted to listing one private DNF package stage"
            );
        }
        if program == TrustedProgram::Sha256sum {
            anyhow::ensure!(
                dnf::trusted_sha256sum_arguments(&arguments),
                "sha256sum is restricted to bounded files in one private DNF stage"
            );
        }
        if program == TrustedProgram::Rm {
            anyhow::ensure!(
                dnf::trusted_remove_stage_arguments(&arguments),
                "rm is restricted to cleaning one private DNF package stage"
            );
        }
        if program == TrustedProgram::RpmDb {
            anyhow::ensure!(
                dnf::trusted_rpmdb_arguments(&arguments),
                "rpmdb is restricted to initializing one isolated DNF key database"
            );
        }
        if program == TrustedProgram::RpmKeys {
            anyhow::ensure!(
                dnf::trusted_rpmkeys_arguments(&arguments),
                "rpmkeys is restricted to pinned-key inventory and staged RPM verification"
            );
        }
        if program == TrustedProgram::Rpm {
            anyhow::ensure!(
                dnf::trusted_rpm_query_arguments(&arguments),
                "rpm query is restricted to installed names or staged package names"
            );
        }
        if program == TrustedProgram::LinuxRootlessPackageSetup {
            anyhow::ensure!(
                matches!(
                    arguments.as_slice(),
                    [TrustedArgument::LinuxRootlessRecipe(_)]
                ),
                "rootless package setup requires one typed recipe binding"
            );
            arguments[0].validate()?;
        }
        Ok(Self {
            host,
            program,
            arguments,
            timeout,
            internal: None,
            progress: None,
            environment: TrustedEnvironment::Isolated,
            #[cfg(test)]
            test_mode: None,
        })
    }

    pub(in crate::app::bootstrap_remediation) fn claude_install(
        target: std::path::PathBuf,
        progress: Sender<ClaudeInstallProgress>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            target.is_absolute(),
            "Claude install target is not absolute"
        );
        Ok(Self {
            host: Host::Linux,
            program: TrustedProgram::ClaudeCodeInstall,
            arguments: Vec::new(),
            timeout: Duration::from_secs(15 * 60),
            internal: Some(InternalInvocation::ClaudeCodeInstall { target }),
            progress: Some(progress),
            environment: TrustedEnvironment::Isolated,
            #[cfg(test)]
            test_mode: None,
        })
    }

    #[cfg(target_os = "linux")]
    pub(in crate::app::bootstrap_remediation::supervisor::process) fn claude_download(
        asset: ClaudeReleaseAsset,
        destination: std::path::PathBuf,
        max_bytes: u64,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            destination.is_absolute()
                && max_bytes > 0
                && max_bytes <= 512 * 1024 * 1024
                && timeout > Duration::ZERO
                && timeout <= MAX_CLAUDE_RUNTIME,
            "Claude release download parameters are outside policy"
        );
        Ok(Self {
            host: Host::Linux,
            program: TrustedProgram::ClaudeCodeDownload,
            arguments: Vec::new(),
            timeout,
            internal: Some(InternalInvocation::ClaudeCodeDownload {
                asset,
                destination,
                max_bytes,
            }),
            progress: None,
            environment: TrustedEnvironment::Isolated,
            #[cfg(test)]
            test_mode: None,
        })
    }

    pub(in crate::app::bootstrap_remediation) fn new_for_current_user_systemd(
        host: Host,
        program: TrustedProgram,
        arguments: Vec<TrustedArgument>,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            host == Host::Linux,
            "current-user systemd operations are Linux-only"
        );
        let mut invocation = Self::new(host, program, arguments, timeout)?;
        invocation.environment = TrustedEnvironment::CurrentUserSystemd;
        Ok(invocation)
    }

    pub(in crate::app::bootstrap_remediation) fn native_elevation_preflight(
        &self,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.host == Host::Linux,
            "native package elevation preflight is Linux-only"
        );
        let (program, version_argument) = match self.program {
            TrustedProgram::AptGet => (TrustedProgram::AptGet, TrustedArgument::AptGetVersion),
            TrustedProgram::LinuxRootlessPackageSetup => match self.rootless_package_recipe()? {
                LinuxRootlessRecipeV1::Debian13Amd64
                | LinuxRootlessRecipeV1::Ubuntu2404Amd64
                | LinuxRootlessRecipeV1::PopOs2404Amd64 => {
                    (TrustedProgram::AptGet, TrustedArgument::AptGetVersion)
                }
                LinuxRootlessRecipeV1::Fedora45Amd64 => {
                    (TrustedProgram::Dnf, TrustedArgument::DnfVersion)
                }
            },
            TrustedProgram::Usermod => (TrustedProgram::Usermod, TrustedArgument::UsermodVersion),
            _ => anyhow::bail!("native elevation preflight is unsupported for this program"),
        };
        Ok(Self {
            host: self.host,
            program,
            arguments: TrustedArgument::version_probe_arguments(version_argument),
            timeout: self.timeout,
            internal: None,
            progress: None,
            environment: self.environment,
            #[cfg(test)]
            test_mode: None,
        })
    }

    pub(in crate::app::bootstrap_remediation::supervisor) fn binding_digest(
        &self,
    ) -> anyhow::Result<String> {
        let mut digest = Sha256::new();
        digest.update(format!(
            "koolade-remediation-invocation-v1|{:?}|{:?}|{:?}|{:?}",
            self.host, self.program, self.timeout, self.environment
        ));
        for argument in &self.arguments {
            let value = argument.binding_value();
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        if let Some(internal) = &self.internal {
            let value = match internal {
                InternalInvocation::ClaudeCodeInstall { target } => {
                    format!("internal:claude_install:{}", target.display())
                }
                #[cfg(target_os = "linux")]
                InternalInvocation::ClaudeCodeDownload {
                    asset,
                    destination,
                    max_bytes,
                } => format!(
                    "internal:claude_download:{asset:?}:{}:{max_bytes}",
                    destination.display()
                ),
            };
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        Ok(format!("{:x}", digest.finalize()))
    }

    #[cfg(test)]
    pub(in crate::app::bootstrap_remediation) fn program_for_test(&self) -> TrustedProgram {
        self.program
    }

    #[cfg(test)]
    pub(in crate::app::bootstrap_remediation) fn arguments_for_test(&self) -> Vec<String> {
        self.arguments
            .iter()
            .map(|argument| argument.as_os_string().to_string_lossy().into_owned())
            .collect()
    }

    #[cfg(test)]
    pub(in crate::app::bootstrap_remediation) fn uses_current_user_systemd_for_test(&self) -> bool {
        self.environment == TrustedEnvironment::CurrentUserSystemd
    }

    #[cfg(test)]
    pub(in crate::app::bootstrap_remediation) fn binding_digest_for_test(
        &self,
    ) -> anyhow::Result<String> {
        self.binding_digest()
    }

    #[cfg(test)]
    pub(in crate::app::bootstrap_remediation::supervisor::process) fn test_helper(
        test_name: &str,
        mode: &str,
        timeout: Duration,
    ) -> Self {
        Self {
            host: Host::Linux,
            program: TrustedProgram::TestHelper,
            arguments: vec![
                TrustedArgument::TestLiteral("--exact".into()),
                TrustedArgument::TestLiteral(test_name.into()),
                TrustedArgument::TestLiteral("--nocapture".into()),
            ],
            timeout,
            internal: None,
            progress: None,
            environment: TrustedEnvironment::Isolated,
            test_mode: Some(mode.into()),
        }
    }
}

fn trusted_docker_repository_create(arguments: &[TrustedArgument]) -> bool {
    matches!(
        arguments,
        [
            TrustedArgument::DockerAptKeyDdInput(_),
            TrustedArgument::DdDockerAptKeyOutput,
            TrustedArgument::DdOutputExclusive,
            TrustedArgument::DdSync,
            TrustedArgument::DdStatusNone
        ] | [
            TrustedArgument::DockerAptSourcesDdInput(_),
            TrustedArgument::DdDockerAptSourcesOutput,
            TrustedArgument::DdOutputExclusive,
            TrustedArgument::DdSync,
            TrustedArgument::DdStatusNone
        ] | [
            TrustedArgument::DockerDnfKeyDdInput(_),
            TrustedArgument::DdDockerDnfKeyOutput,
            TrustedArgument::DdOutputExclusive,
            TrustedArgument::DdSync,
            TrustedArgument::DdStatusNone
        ] | [
            TrustedArgument::DockerDnfRepoDdInput(_),
            TrustedArgument::DdDockerDnfRepoOutput,
            TrustedArgument::DdOutputExclusive,
            TrustedArgument::DdSync,
            TrustedArgument::DdStatusNone
        ]
    )
}
