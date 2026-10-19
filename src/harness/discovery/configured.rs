use super::ProbeOutput;
use std::time::Duration;

/// Runs a provider probe with explicitly selected, non-secret config-path overrides.
pub(crate) fn run_with_environment(
    argv: &[String],
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    excluded_environment: &[&str],
    environment_overrides: &[(&str, &str)],
) -> anyhow::Result<ProbeOutput> {
    super::run_inner(
        argv,
        timeout,
        stdout_limit,
        stderr_limit,
        excluded_environment,
        environment_overrides,
        false,
    )
}
