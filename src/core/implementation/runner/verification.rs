use super::{Runner, verification_command};
use std::{path::Path, time::Duration};

impl Runner {
    pub(in crate::core::implementation) fn check_storage(&self, path: &Path) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            let existing = path
                .ancestors()
                .find(|candidate| candidate.is_dir())
                .ok_or_else(|| {
                    anyhow::anyhow!("Cannot locate filesystem for {}", path.display())
                })?;
            let output = self.command(existing, "df", &["-Pk", "."])?;
            let available = output
                .lines()
                .last()
                .and_then(|line| line.split_whitespace().nth(3))
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cannot determine available disk space for {}",
                        path.display()
                    )
                })?;
            anyhow::ensure!(
                available >= 1024 * 1024,
                "Insufficient disk space at {}: {} MiB available; at least 1 GiB is required to start implementation or verification. Free rebuildable build caches, then Resume implementation. Existing work is preserved.",
                path.display(),
                available / 1024
            );
        }
        Ok(())
    }

    pub(in crate::core::implementation) fn verify(
        &self,
        cwd: &Path,
        command: &str,
        workspace: &crate::execution::contract::WorkspaceRef,
    ) -> anyhow::Result<String> {
        verification_command::validate(command)?;
        self.check_storage(cwd)?;
        workspace.validate().map_err(|_| {
            anyhow::anyhow!("Verification workspace identity is invalid; refusing host execution")
        })?;
        match self.verify_once(cwd, command, workspace) {
            Ok(output) => Ok(output),
            Err(error) if is_nuget_audit_failure(&error) => {
                self.update(
                    "Refreshing public NuGet vulnerability data, then retrying verification…",
                );
                let timeout = self.remaining()?.min(Duration::from_secs(90));
                crate::harness::refresh_nuget_audit_cache(timeout).map_err(|refresh_error| {
                    anyhow::anyhow!("{error}\nKool.ad/e could not refresh the public NuGet audit cache: {refresh_error:#}")
                })?;
                self.verify_once(cwd, command, workspace).map_err(|retry_error| {
                    anyhow::anyhow!("{retry_error}\nKool.ad/e refreshed the public NuGet audit cache and retried verification, but the feed error remains.")
                })
            }
            Err(error) => Err(error),
        }
    }

    fn verify_once(
        &self,
        cwd: &Path,
        command: &str,
        workspace: &crate::execution::contract::WorkspaceRef,
    ) -> anyhow::Result<String> {
        let remaining = self.remaining()?;
        #[cfg(test)]
        if let Some(executor) = super::test_verification_executor() {
            return executor(cwd, command, workspace);
        }
        let profile = if cwd.join("Cargo.toml").is_file() {
            crate::execution::docker::images::RuntimeProfile::Rust
        } else if cwd.join("package.json").is_file() {
            crate::execution::docker::images::RuntimeProfile::Node
        } else if cwd.join("pyproject.toml").is_file() || cwd.join("requirements.txt").is_file() {
            crate::execution::docker::images::RuntimeProfile::Python
        } else if cwd.join("go.mod").is_file() {
            crate::execution::docker::images::RuntimeProfile::Go
        } else {
            crate::execution::docker::images::RuntimeProfile::General
        };
        let session =
            crate::execution::session::ExecutionSession::connect_local_with_authorization(
                workspace.clone(),
                cwd,
                crate::execution::contract::ExecutionModeV1::Verification,
                profile,
                crate::execution::contract::ResourceBudgetV1 {
                    cpu_millis: 4_000,
                    memory_bytes: 4 * 1024 * 1024 * 1024,
                    process_count: 256,
                    temporary_disk_bytes: 4 * 1024 * 1024 * 1024,
                    output_bytes: 8 * 1024 * 1024,
                },
                self.authorization_verifier.clone(),
            )
            .map_err(|error| {
                anyhow::anyhow!("Docker verification is unavailable: {}", error.message)
            })?;
        let output = session
            .run_tool_command(command, remaining.min(Duration::from_secs(1200)))
            .map_err(|error| anyhow::anyhow!("Docker verification failed: {}", error.message))?;
        let mut text = output.stdout;
        if !output.stderr.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&output.stderr);
        }
        anyhow::ensure!(
            output.result.exit_code == Some(0)
                && output.result.failure.is_none()
                && !output.result.timed_out
                && !output.result.cancelled,
            "Docker verification command failed: {command}\n{text}"
        );
        Ok(text)
    }

    pub(in crate::core::implementation) fn workspace_ref_for(
        &self,
        state: &super::super::Implementation,
    ) -> anyhow::Result<crate::execution::contract::WorkspaceRef> {
        let project_id = state
            .workspace_uid
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Verification project identity is missing"))?;
        let task_id = state
            .task_uid
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Verification task identity is missing"))?;
        crate::execution::contract::validate_identifier(project_id)
            .map_err(|_| anyhow::anyhow!("Verification project identity is invalid"))?;
        crate::execution::contract::validate_identifier(task_id)
            .map_err(|_| anyhow::anyhow!("Verification task identity is invalid"))?;
        let source_sha = state
            .source_commit
            .as_deref()
            .filter(|sha| crate::execution::contract::is_valid_sha(sha))
            .or_else(|| {
                crate::execution::contract::is_valid_sha(&state.base_commit)
                    .then_some(state.base_commit.as_str())
            })
            .ok_or_else(|| anyhow::anyhow!("Saved verification source snapshot is missing"))?
            .to_owned();
        Ok(crate::execution::contract::WorkspaceRef {
            project_id: project_id.to_owned(),
            task_id: task_id.to_owned(),
            source_sha,
        })
    }

    #[cfg(test)]
    pub(in crate::core::implementation) fn verify_with_executor(
        &self,
        cwd: &Path,
        command: &str,
        execute: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<String> {
        verification_command::validate(command)?;
        self.check_storage(cwd)?;
        self.remaining()?;
        execute(command)
    }
}

fn is_nuget_audit_failure(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("error nu1900:")
}

#[cfg(test)]
mod tests {
    use super::is_nuget_audit_failure;

    #[test]
    fn refresh_retry_is_limited_to_fatal_nuget_audit_errors() {
        assert!(is_nuget_audit_failure(&anyhow::anyhow!(
            "dotnet build failed: error NU1900: audit feed unavailable"
        )));
        assert!(!is_nuget_audit_failure(&anyhow::anyhow!(
            "dotnet build failed: warning NU1900: audit feed unavailable"
        )));
        assert!(!is_nuget_audit_failure(&anyhow::anyhow!(
            "dotnet build failed: error CS0246: missing type"
        )));
    }
}
