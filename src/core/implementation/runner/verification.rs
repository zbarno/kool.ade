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
    ) -> anyhow::Result<String> {
        verification_command::validate(command)?;
        self.check_storage(cwd)?;
        match self.verify_once(cwd, command) {
            Ok(output) => Ok(output),
            Err(error) if is_nuget_audit_failure(&error) => {
                self.update(
                    "Refreshing public NuGet vulnerability data, then retrying verification…",
                );
                let timeout = self.remaining()?.min(Duration::from_secs(90));
                crate::harness::refresh_nuget_audit_cache(timeout).map_err(|refresh_error| {
                    anyhow::anyhow!(
                        "{error}\nKool.ad/e could not refresh the public NuGet audit cache: {refresh_error:#}"
                    )
                })?;
                self.verify_once(cwd, command).map_err(|retry_error| {
                    anyhow::anyhow!(
                        "{retry_error}\nKool.ad/e refreshed the public NuGet audit cache and retried verification, but the feed error remains."
                    )
                })
            }
            Err(error) => Err(error),
        }
    }

    fn verify_once(&self, cwd: &Path, command: &str) -> anyhow::Result<String> {
        cwd.to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 task repository path"))?;
        let mut sandbox = if let Some(source) = &self.runtime_config_source {
            crate::harness::pi_sandbox::Sandbox::new_for_task_repository(cwd, source)?
        } else {
            crate::harness::pi_sandbox::Sandbox::new(cwd)?
        };
        sandbox.mount_npm_cache(&crate::harness::prepared_npm_cache_path(cwd)?)?;
        sandbox.mount_cargo_cache(&crate::harness::prepared_cargo_cache_path()?)?;
        let args = sandbox.command_args("/bin/sh", command);
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        self.command_clean_env(
            cwd,
            sandbox
                .bwrap
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Non-UTF8 bubblewrap path"))?,
            &args,
        )
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
