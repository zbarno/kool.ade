use sha2::{Digest, Sha256};
use std::{ffi::OsString, time::Duration};

use super::{
    artifact::VerifiedArtifact,
    catalog::{NativePackage, TrustedFlag, TrustedProgram},
};
use crate::app::bootstrap_repair_policy::Host;

const MAX_RUNTIME: Duration = Duration::from_secs(120);

pub(crate) enum TrustedArgument {
    Flag(TrustedFlag),
    Package(NativePackage),
    VerifiedArtifact(VerifiedArtifact),
    #[cfg(test)]
    TestLiteral(String),
}

impl TrustedArgument {
    pub(in crate::app::bootstrap_remediation::supervisor::process) fn as_os_string(
        &self,
    ) -> OsString {
        match self {
            Self::Flag(flag) => flag.as_str().into(),
            Self::Package(package) => package.as_str().into(),
            Self::VerifiedArtifact(artifact) => artifact.execution_path(),
            #[cfg(test)]
            Self::TestLiteral(value) => value.into(),
        }
    }

    pub(in crate::app::bootstrap_remediation::supervisor::process) fn validate(
        &self,
    ) -> anyhow::Result<()> {
        if let Self::VerifiedArtifact(artifact) = self {
            artifact.validate()?;
        }
        Ok(())
    }
}

impl TrustedFlag {
    fn as_str(self) -> &'static str {
        match self {
            Self::AptInstall => "install",
            Self::AssumeYes => "--assume-yes",
            Self::Quiet => "--quiet",
            Self::MsiInstall => "/i",
            Self::NoRestart => "/norestart",
        }
    }
}

pub(crate) struct TrustedInvocation {
    pub(in crate::app::bootstrap_remediation::supervisor::process) host: Host,
    pub(in crate::app::bootstrap_remediation::supervisor::process) program: TrustedProgram,
    pub(in crate::app::bootstrap_remediation::supervisor::process) arguments: Vec<TrustedArgument>,
    pub(in crate::app::bootstrap_remediation::supervisor::process) timeout: Duration,
    #[cfg(test)]
    pub(in crate::app::bootstrap_remediation::supervisor::process) test_mode: Option<String>,
}

impl TrustedInvocation {
    pub(crate) fn host(&self) -> Host {
        self.host
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
        anyhow::ensure!(
            arguments.len() <= 48,
            "too many fixed remediation arguments"
        );
        anyhow::ensure!(
            program.matches(host),
            "trusted program does not match the planned host"
        );
        Ok(Self {
            host,
            program,
            arguments,
            timeout,
            #[cfg(test)]
            test_mode: None,
        })
    }

    pub(in crate::app::bootstrap_remediation::supervisor) fn binding_digest(
        &self,
    ) -> anyhow::Result<String> {
        let mut digest = Sha256::new();
        digest.update(format!(
            "koolade-remediation-invocation-v1|{:?}|{:?}|{:?}",
            self.host, self.program, self.timeout
        ));
        for argument in &self.arguments {
            let value = match argument {
                TrustedArgument::Flag(flag) => format!("flag:{flag:?}"),
                TrustedArgument::Package(package) => format!("package:{package:?}"),
                TrustedArgument::VerifiedArtifact(artifact) => {
                    format!("artifact:{}:{}", artifact.path.display(), artifact.sha256)
                }
                #[cfg(test)]
                TrustedArgument::TestLiteral(value) => format!("test:{value}"),
            };
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        Ok(format!("{:x}", digest.finalize()))
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
            test_mode: Some(mode.into()),
        }
    }
}
