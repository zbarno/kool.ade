use std::io::Read;
use std::sync::mpsc::Sender;

use sha2::{Digest, Sha256};

use crate::app::bootstrap_remediation::{
    claude_code_install::ClaudeInstallProgress,
    plan::RemediationPlan,
    supervisor::{ReadinessVerifier, VerificationReceipt},
};

pub(crate) struct ClaudeCodeReadinessVerifier {
    progress: Sender<ClaudeInstallProgress>,
}

impl ClaudeCodeReadinessVerifier {
    pub(crate) fn new(progress: Sender<ClaudeInstallProgress>) -> Self {
        Self { progress }
    }
}

impl ReadinessVerifier for ClaudeCodeReadinessVerifier {
    fn verify(&self, plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt> {
        let _ = self
            .progress
            .send(ClaudeInstallProgress::FreshProviderProbe);
        anyhow::ensure!(
            plan.verification_probe_ids == ["cli:claude:install"],
            "Claude install verification probe changed"
        );
        let expected_path = plan
            .install_target
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Claude install target is missing"))?;
        let report = crate::harness::ClaudeHarness::probe_report();
        let binary = report
            .binary
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("fresh Claude discovery did not find an executable"))?;
        anyhow::ensure!(
            binary.canonicalize()? == std::path::Path::new(expected_path).canonicalize()?,
            "fresh Claude discovery selected a different executable"
        );
        anyhow::ensure!(
            installed_digest(binary)? == super::recipe::BINARY_SHA256,
            "fresh Claude discovery target no longer matches the pinned binary"
        );
        anyhow::ensure!(
            report.version.as_deref() == Some(super::recipe::RELEASE)
                && install_probe_succeeded(report.readiness),
            "fresh Claude discovery did not confirm the pinned installed release"
        );
        anyhow::ensure!(
            installed_digest(binary)? == super::recipe::BINARY_SHA256,
            "Claude executable changed during fresh discovery"
        );
        let observed_at_unix_ms = super::super::approval::now_unix_ms()?;
        let evidence = format!(
            "claude-install-v1|{}|{}|{:?}|{}",
            binary.display(),
            report.version.as_deref().unwrap_or_default(),
            report.readiness,
            observed_at_unix_ms
        );
        Ok(VerificationReceipt {
            probe_ids: plan.verification_probe_ids.clone(),
            observed_at_unix_ms,
            evidence_digest: format!("{:x}", Sha256::digest(evidence.as_bytes())),
            ready: true,
        })
    }
}

fn installed_digest(path: &std::path::Path) -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    let mut file = {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let metadata = std::fs::symlink_metadata(path)?;
        anyhow::ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o022 == 0
                && metadata.mode() & 0o111 != 0,
            "Claude install target is not a safe user-local executable"
        );
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?
    };
    #[cfg(not(target_os = "linux"))]
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn install_probe_succeeded(readiness: crate::harness::claude_harness::ClaudeReadiness) -> bool {
    matches!(
        readiness,
        crate::harness::claude_harness::ClaudeReadiness::Ready
            | crate::harness::claude_harness::ClaudeReadiness::AuthenticationRequired
            | crate::harness::claude_harness::ClaudeReadiness::AuthenticationUnverified
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::claude_harness::ClaudeReadiness;

    #[test]
    fn install_verification_does_not_claim_that_authentication_succeeded() {
        assert!(install_probe_succeeded(ClaudeReadiness::Ready));
        assert!(install_probe_succeeded(
            ClaudeReadiness::AuthenticationRequired
        ));
        assert!(install_probe_succeeded(
            ClaudeReadiness::AuthenticationUnverified
        ));
        assert!(!install_probe_succeeded(ClaudeReadiness::Missing));
        assert!(!install_probe_succeeded(ClaudeReadiness::Unusable));
        assert!(!install_probe_succeeded(ClaudeReadiness::Unsupported));
    }
}
