use serde::Serialize;

mod platform;
use sha2::{Digest, Sha256};

use crate::app::bootstrap_repair_policy::Host;

/// Hashed identity for the local OS installation and effective user.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub(crate) struct HostIdentity {
    host: Host,
    os_version: String,
    machine_digest: String,
    principal_digest: String,
    /// Local consent display only; excluded from the serialized binding.
    #[serde(skip)]
    machine_label: String,
    /// Local consent display only; excluded from the serialized binding.
    #[serde(skip)]
    principal_label: String,
}

impl HostIdentity {
    #[cfg(test)]
    pub(crate) fn from_facts(
        host: Host,
        os_version: &str,
        machine_id: &str,
        principal_id: &str,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !machine_id.is_empty() && machine_id.len() <= 256,
            "invalid machine identity"
        );
        anyhow::ensure!(
            !principal_id.is_empty() && principal_id.len() <= 256,
            "invalid user identity"
        );
        validate_os_version(os_version)?;
        let machine_digest = fingerprint(&("koolade-remediation-machine-v1", host, machine_id))?;
        let principal_digest = fingerprint(&(
            "koolade-remediation-principal-v1",
            &machine_digest,
            principal_id,
        ))?;
        let identity = Self {
            host,
            os_version: os_version.to_owned(),
            machine_digest,
            principal_digest,
            machine_label: sanitize_display_label(machine_id),
            principal_label: sanitize_display_label(principal_id),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub(crate) fn current() -> anyhow::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            let machine_id = super::super::bootstrap_approval::linux_machine_id()?;
            let principal = unsafe { libc::geteuid() }.to_string();
            let os_version = platform::linux_os_version()?;
            let machine_label = platform::unix_machine_label()?;
            let principal_label = platform::unix_principal_label()?;
            Self::from_parts(
                Host::Linux,
                &os_version,
                &machine_id,
                &principal,
                &machine_label,
                &principal_label,
            )
        }
        #[cfg(target_os = "macos")]
        {
            let machine_id = platform::macos_machine_id()?;
            let principal = unsafe { libc::geteuid() }.to_string();
            let os_version = platform::macos_os_version()?;
            let machine_label = platform::unix_machine_label()?;
            let principal_label = platform::unix_principal_label()?;
            Self::from_parts(
                Host::MacOs,
                &os_version,
                &machine_id,
                &principal,
                &machine_label,
                &principal_label,
            )
        }
        #[cfg(target_os = "windows")]
        {
            let machine_id = platform::windows_machine_id()?;
            let principal = platform::windows_principal_id()?;
            let os_version = platform::windows_os_version()?;
            let machine_label = platform::windows_machine_label()?;
            let principal_label = platform::windows_principal_label()?;
            Self::from_parts(
                Host::Windows,
                &os_version,
                &machine_id,
                &principal,
                &machine_label,
                &principal_label,
            )
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        anyhow::bail!("machine remediation identity is unsupported on this host")
    }

    fn from_parts(
        host: Host,
        os_version: &str,
        machine_id: &str,
        principal_id: &str,
        machine_label: &str,
        principal_label: &str,
    ) -> anyhow::Result<Self> {
        validate_os_version(os_version)?;
        anyhow::ensure!(
            !machine_id.is_empty() && machine_id.len() <= 256,
            "invalid machine identity"
        );
        anyhow::ensure!(
            !principal_id.is_empty() && principal_id.len() <= 256,
            "invalid user identity"
        );
        let machine_digest = fingerprint(&("koolade-remediation-machine-v1", host, machine_id))?;
        let principal_digest = fingerprint(&(
            "koolade-remediation-principal-v1",
            &machine_digest,
            principal_id,
        ))?;
        let identity = Self {
            host,
            os_version: os_version.to_owned(),
            machine_digest,
            principal_digest,
            machine_label: sanitize_display_label(machine_label),
            principal_label: sanitize_display_label(principal_label),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub(crate) fn host(&self) -> Host {
        self.host
    }

    pub(crate) fn os_version(&self) -> &str {
        &self.os_version
    }

    pub(crate) fn matches_current(&self) -> anyhow::Result<bool> {
        Ok(self.digest()? == Self::current()?.digest()?)
    }

    pub(crate) fn digest(&self) -> anyhow::Result<String> {
        fingerprint(self)
    }

    pub(crate) fn machine_fingerprint(&self) -> &str {
        &self.machine_digest
    }

    pub(crate) fn principal_fingerprint(&self) -> &str {
        &self.principal_digest
    }

    pub(crate) fn machine_label(&self) -> &str {
        &self.machine_label
    }

    pub(crate) fn principal_label(&self) -> &str {
        &self.principal_label
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            is_sha256_hex(&self.machine_digest) && is_sha256_hex(&self.principal_digest),
            "machine remediation identity is invalid"
        );
        validate_os_version(&self.os_version)?;
        anyhow::ensure!(
            !self.machine_label.is_empty()
                && self.machine_label.chars().count() <= 128
                && !self.principal_label.is_empty()
                && self.principal_label.chars().count() <= 128,
            "machine or account display label is invalid"
        );
        Ok(())
    }
}

fn sanitize_display_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_os_version(value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 64
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
            }),
        "invalid host OS version"
    );
    Ok(())
}

fn fingerprint<T: Serialize>(value: &T) -> anyhow::Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_principal_hashes_are_domain_separated() {
        let first =
            HostIdentity::from_facts(Host::Linux, "6.12", "machine-one", "user-one").unwrap();
        let other_user =
            HostIdentity::from_facts(Host::Linux, "6.12", "machine-one", "user-two").unwrap();
        let other_machine =
            HostIdentity::from_facts(Host::Linux, "6.12", "machine-two", "user-one").unwrap();
        assert_ne!(first.digest().unwrap(), other_machine.digest().unwrap());
        assert_ne!(first.principal_digest, other_user.principal_digest);
        assert_eq!(first.machine_label(), "machine-one");
        assert_eq!(first.principal_label(), "user-one");
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("machine-one"));
        assert!(!serialized.contains("user-one"));
        first.validate().unwrap();
    }
}
