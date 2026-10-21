use std::path::PathBuf;

const FIXED_REMOTE_COMMAND: &str = "koolade worker serve --stdio";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshWorkerConfig {
    pub stable_id: String,
    pub host_alias: String,
    pub ssh_principal: String,
    pub verified_host_key_fingerprint: String,
    pub known_hosts_path: PathBuf,
    pub ssh_binary: PathBuf,
}

impl SshWorkerConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(valid_identifier(&self.stable_id), "worker ID is invalid");
        anyhow::ensure!(
            !self.host_alias.is_empty()
                && self.host_alias.len() <= 255
                && self.host_alias.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                })
                && !self.host_alias.starts_with('-'),
            "SSH host must be a configured alias"
        );
        anyhow::ensure!(
            valid_ssh_principal(&self.ssh_principal),
            "SSH principal is invalid"
        );
        anyhow::ensure!(
            self.verified_host_key_fingerprint.starts_with("SHA256:")
                && self.verified_host_key_fingerprint.len() == 50
                && self.verified_host_key_fingerprint[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/'),
            "verified SSH host-key fingerprint is invalid"
        );
        anyhow::ensure!(
            !self.ssh_binary.as_os_str().is_empty(),
            "SSH executable is missing"
        );
        anyhow::ensure!(
            self.known_hosts_path.is_file(),
            "approved SSH known_hosts file is unavailable"
        );
        Ok(())
    }

    /// Confirm the approved fingerprint exists for this stable worker identity.
    /// OpenSSH still performs the handshake check; this prevents a changed or
    /// mismatched known_hosts entry from silently rebinding the worker.
    pub(crate) fn verify_known_host(&self) -> anyhow::Result<VerifiedSshWorker> {
        self.validate()?;
        let registration = crate::persistence::remote_workers::get(&self.stable_id)?
            .ok_or_else(|| anyhow::anyhow!("remote worker is not enrolled in local settings"))?;
        anyhow::ensure!(
            registration.ssh_host_alias == self.host_alias
                && registration.ssh_principal == self.ssh_principal
                && registration.host_key_fingerprint == self.verified_host_key_fingerprint,
            "remote worker registration does not match the SSH identity"
        );
        anyhow::ensure!(registration.enabled, "remote worker is disabled locally");
        let output = std::process::Command::new("ssh-keygen")
            .args(["-F", &self.stable_id, "-f"])
            .arg(&self.known_hosts_path)
            .arg("-l")
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "approved SSH host key is absent from known_hosts"
        );
        let fingerprints = String::from_utf8_lossy(&output.stdout);
        anyhow::ensure!(
            known_host_fingerprint_matches(&fingerprints, &self.verified_host_key_fingerprint,),
            "SSH host key does not match the trusted worker fingerprint"
        );
        Ok(VerifiedSshWorker {
            config: self.clone(),
        })
    }

    fn argv(&self) -> Vec<String> {
        vec![
            self.ssh_binary.to_string_lossy().into_owned(),
            "-T".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "StrictHostKeyChecking=yes".into(),
            "-o".into(),
            "UpdateHostKeys=no".into(),
            "-o".into(),
            format!("UserKnownHostsFile={}", self.known_hosts_path.display()),
            "-o".into(),
            "GlobalKnownHostsFile=/dev/null".into(),
            "-o".into(),
            "ProxyCommand=none".into(),
            "-o".into(),
            "ProxyJump=none".into(),
            "-o".into(),
            "ClearAllForwardings=yes".into(),
            "-o".into(),
            "ForwardAgent=no".into(),
            "-o".into(),
            "ControlMaster=no".into(),
            "-o".into(),
            "PermitLocalCommand=no".into(),
            "-o".into(),
            "RequestTTY=no".into(),
            "-o".into(),
            "ConnectTimeout=10".into(),
            "-o".into(),
            "ServerAliveInterval=15".into(),
            "-o".into(),
            "ServerAliveCountMax=3".into(),
            "-o".into(),
            format!("HostKeyAlias={}", self.stable_id),
            format!("{}@{}", self.ssh_principal, self.host_alias),
            FIXED_REMOTE_COMMAND.into(),
        ]
    }
}

fn known_host_fingerprint_matches(output: &str, expected: &str) -> bool {
    output.lines().any(|line| {
        line.split_whitespace()
            .any(|field| field.starts_with("SHA256:") && field == expected)
    })
}

/// A worker whose pinned public key was found in the selected known_hosts file.
/// Only this verified value can produce connection arguments.
pub(crate) struct VerifiedSshWorker {
    config: SshWorkerConfig,
}

impl VerifiedSshWorker {
    /// Construct argv without a shell; callers must execute it directly.
    pub(crate) fn argv(&self) -> Vec<String> {
        self.config.argv()
    }

    /// Start the fixed worker stream over OpenSSH. Standard input/output are
    /// reserved for the framed protocol; stderr remains separate.
    pub(crate) fn spawn(&self) -> std::io::Result<std::process::Child> {
        let argv = self.argv();
        let mut command = std::process::Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env_remove("SSH_ASKPASS")
            .env_remove("DISPLAY")
            .env_remove("GIT_SSH_COMMAND");
        command.spawn()
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_ssh_principal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SshWorkerConfig {
        SshWorkerConfig {
            stable_id: "trusted-worker-1".into(),
            host_alias: "build-host".into(),
            ssh_principal: "worker-user".into(),
            verified_host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
            known_hosts_path: PathBuf::from("/tmp/koolade-known-hosts-test"),
            ssh_binary: PathBuf::from("/usr/bin/ssh"),
        }
    }

    #[test]
    fn ssh_argv_requires_host_verification_and_uses_fixed_remote_command() {
        let mut config = config();
        config.known_hosts_path = std::env::temp_dir();
        let argv = config.argv();
        assert_eq!(argv[1], "-T");
        assert!(
            argv.windows(2)
                .any(|pair| pair == ["-o", "StrictHostKeyChecking=yes"])
        );
        assert!(argv.iter().any(|arg| arg == "ProxyCommand=none"));
        assert_eq!(argv.last().unwrap(), FIXED_REMOTE_COMMAND);
        assert!(!argv.iter().any(|argument| argument.contains(";")));
    }

    #[test]
    fn rejects_shell_arguments_and_invalid_host_fingerprints() {
        let mut malicious = config();
        malicious.known_hosts_path = std::env::temp_dir();
        malicious.host_alias = "host; touch /tmp/pwned".into();
        assert!(malicious.validate().is_err());

        let mut invalid_principal = config();
        invalid_principal.known_hosts_path = std::env::temp_dir();
        invalid_principal.ssh_principal = "worker@other-host".into();
        assert!(invalid_principal.validate().is_err());

        let mut unverified = config();
        unverified.known_hosts_path = std::env::temp_dir();
        unverified.verified_host_key_fingerprint = "SHA256:unknown".into();
        assert!(unverified.validate().is_err());
    }

    #[test]
    fn known_hosts_fingerprint_must_match_the_approved_worker_key() {
        let trusted = format!("SHA256:{}", "A".repeat(43));
        assert!(known_host_fingerprint_matches(
            &format!("trusted-worker-1 ED25519 {trusted} (ED25519)\n"),
            &trusted
        ));
        assert!(known_host_fingerprint_matches(
            &format!("256 {trusted} trusted-worker-1 (ED25519)\n"),
            &trusted
        ));
        assert!(!known_host_fingerprint_matches(
            &format!("256 SHA256:{} trusted-worker-1 (ED25519)\n", "B".repeat(43)),
            &trusted
        ));
        assert!(!known_host_fingerprint_matches(
            "known_hosts has no matching worker identity\n",
            &trusted
        ));
    }
}
