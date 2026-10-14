use super::CliProvider;
use std::path::{Path, PathBuf};

mod probe;
mod telemetry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProfileRequirements {
    pub minimum_version: Option<(u64, u64, u64)>,
    pub capabilities: &'static [&'static str],
    pub help_commands: &'static [&'static [&'static str]],
    pub(crate) launch_tokens: &'static [&'static str],
    pub(crate) environment_keys: &'static [&'static str],
}

/// Evidence returned by the readiness probe for the exact executable that is
/// about to run. Capabilities are populated only after the corresponding
/// adapter probe has validated them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileProbeEvidence {
    provider: CliProvider,
    executable: PathBuf,
    version: String,
    capabilities: Vec<String>,
}

impl ProfileProbeEvidence {
    pub(crate) fn probe(
        provider: CliProvider,
        executable: &Path,
        version: String,
    ) -> anyhow::Result<Self> {
        #[cfg(test)]
        let capabilities: Vec<String> = provider
            .profile_requirements()
            .capabilities
            .iter()
            .map(|value| (*value).to_owned())
            .collect();
        #[cfg(not(test))]
        let capabilities = probe::verified_capabilities(provider, executable, &version)?;
        let capability_refs = capabilities.iter().map(String::as_str).collect::<Vec<_>>();
        Self::validated(provider, executable, version, &capability_refs)
    }

    pub(crate) fn validated(
        provider: CliProvider,
        executable: &Path,
        version: String,
        capabilities: &[&str],
    ) -> anyhow::Result<Self> {
        let evidence = Self {
            provider,
            executable: executable.to_path_buf(),
            version,
            capabilities: capabilities
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.executable.is_absolute(),
            "provider executable identity is not absolute"
        );
        anyhow::ensure!(
            !self.version.trim().is_empty(),
            "provider version evidence is empty"
        );
        let requirements = self.provider.profile_requirements();
        let actual = parse_version(&self.version).ok_or_else(|| {
            anyhow::anyhow!("{} version evidence is not parseable", self.provider.name())
        })?;
        if let Some(minimum) = requirements.minimum_version {
            anyhow::ensure!(
                actual >= minimum,
                "{} profile requires version {}.{}.{} or later",
                self.provider.name(),
                minimum.0,
                minimum.1,
                minimum.2
            );
        }
        let available = self
            .capabilities
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let missing = requirements.missing_capabilities(&available);
        anyhow::ensure!(
            missing.is_empty(),
            "{} profile probe is missing capabilities: {}",
            self.provider.name(),
            missing.join(", ")
        );
        Ok(())
    }
}

/// Owns the final argv and environment after profile validation. Callers
/// cannot mutate launch inputs between validation and process creation.
pub(crate) struct ValidatedProviderLaunch {
    evidence: ProfileProbeEvidence,
    argv: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    seal: String,
}

impl ValidatedProviderLaunch {
    #[cfg(test)]
    pub(crate) fn add_test_argv_drift(&mut self) {
        self.argv.push("--unexpected-mode".into());
    }

    #[cfg(test)]
    pub(crate) fn add_test_environment_drift(&mut self) {
        self.env.push(("PROFILE_DRIFT".into(), "1".into()));
    }

    pub(crate) fn bind(
        evidence: ProfileProbeEvidence,
        argv: Vec<String>,
        cwd: &Path,
        explicit_env: Vec<(String, String)>,
    ) -> anyhow::Result<Self> {
        let env =
            crate::harness::pi_proc::provider_environment(evidence.provider, cwd, &explicit_env);
        let launch = Self {
            evidence,
            argv,
            cwd: cwd.to_path_buf(),
            env,
            seal: String::new(),
        };
        launch.validate_profile()?;
        let mut launch = launch;
        launch.seal = launch.fingerprint()?;
        Ok(launch)
    }

    pub(crate) fn spawn_with_input(
        self,
        cwd: &Path,
        input: Option<String>,
    ) -> anyhow::Result<crate::harness::pi_proc::ChildTask> {
        self.validate_profile()?;
        anyhow::ensure!(
            cwd == self.cwd,
            "provider launch working directory changed after validation"
        );
        anyhow::ensure!(
            self.seal == self.fingerprint()?,
            "provider launch profile changed after validation"
        );
        crate::harness::pi_proc::spawn_with_input_clear_env(&self.argv, &self.cwd, input, &self.env)
    }

    pub(crate) fn telemetry(
        &self,
        prompt_role_version: Option<&str>,
    ) -> crate::harness::CleanLaunchTelemetry {
        telemetry::for_launch(self, prompt_role_version)
    }

    fn validate_profile(&self) -> anyhow::Result<()> {
        self.evidence.validate()?;
        let executable = self
            .argv
            .first()
            .ok_or_else(|| anyhow::anyhow!("provider argv is empty"))?;
        anyhow::ensure!(
            Path::new(executable) == self.evidence.executable,
            "provider launch executable does not match its validated probe"
        );
        let missing = self
            .evidence
            .provider
            .profile_requirements()
            .launch_tokens
            .iter()
            .filter(|token| {
                if token.ends_with('=') {
                    !self.argv.iter().any(|argument| {
                        argument.starts_with(**token) && argument.len() > token.len()
                    })
                } else {
                    !self.argv.iter().any(|argument| argument == **token)
                }
            })
            .copied()
            .collect::<Vec<_>>();
        let missing_env = self
            .evidence
            .provider
            .profile_requirements()
            .environment_keys
            .iter()
            .filter(|key| {
                !self
                    .env
                    .iter()
                    .any(|(name, value)| name == **key && !value.is_empty())
            })
            .copied()
            .collect::<Vec<_>>();
        anyhow::ensure!(
            missing.is_empty() && missing_env.is_empty(),
            "{} launch profile is missing required arguments [{}] or environment entries [{}]",
            self.evidence.provider.name(),
            missing.join(", "),
            missing_env.join(", "),
        );
        Ok(())
    }

    fn fingerprint(&self) -> anyhow::Result<String> {
        let evidence = &self.evidence;
        let bytes = serde_json::to_vec(&(
            evidence.provider.name(),
            &evidence.executable,
            &evidence.version,
            &evidence.capabilities,
            &self.argv,
            &self.cwd,
            &self.env,
        ))?;
        use sha2::Digest;
        Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
    }
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let start = value.find(|character: char| character.is_ascii_digit())?;
    let version = value[start..]
        .split_whitespace()
        .next()?
        .trim_start_matches('v')
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .next()?;
    let mut parts = version.split('.').map(str::parse::<u64>);
    Some((
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ))
}

impl ProfileRequirements {
    #[cfg(test)]
    pub(crate) fn launch_tokens(self) -> &'static [&'static str] {
        self.launch_tokens
    }

    #[cfg(test)]
    pub(crate) fn environment_keys(self) -> &'static [&'static str] {
        self.environment_keys
    }

    #[cfg(test)]
    pub(crate) fn requires_launch_token(self, token: &str) -> bool {
        self.launch_tokens.contains(&token)
    }

    #[cfg(test)]
    pub(crate) fn requires_environment_key(self, key: &str) -> bool {
        self.environment_keys.contains(&key)
    }

    #[cfg(test)]
    pub(crate) fn supports_version(self, version: (u64, u64, u64)) -> bool {
        self.minimum_version
            .is_none_or(|minimum| version >= minimum)
    }

    pub(crate) fn missing_capabilities(self, available: &[&str]) -> Vec<&'static str> {
        self.capabilities
            .iter()
            .copied()
            .filter(|required| !available.contains(required))
            .collect()
    }
}
