use super::{Runner, identity::sanitize_remote};
use std::path::Path;

pub(super) fn effective_remote_url(
    repo: &Path,
    push: bool,
    runner: &Runner,
) -> anyhow::Result<Option<String>> {
    let args: &[&str] = if push {
        &["remote", "get-url", "--push", "--all", "origin"]
    } else {
        &["remote", "get-url", "--all", "origin"]
    };
    match runner.git(repo, args) {
        Ok(value) => {
            let values = value
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            anyhow::ensure!(
                values.len() == 1,
                "Repository origin must have exactly one effective {} URL",
                if push { "push" } else { "fetch" }
            );
            Ok(Some(sanitize_remote(values[0])?))
        }
        Err(_) => {
            runner.remaining()?;
            Ok(None)
        }
    }
}

pub(super) fn configured_value(
    repo: &Path,
    key: &str,
    runner: &Runner,
) -> anyhow::Result<Option<String>> {
    configured_raw_value(repo, key, runner)?
        .map(|value| sanitize_remote(&value))
        .transpose()
}

pub(super) fn configured_raw_value(
    repo: &Path,
    key: &str,
    runner: &Runner,
) -> anyhow::Result<Option<String>> {
    match runner.git(repo, &["config", "--get-all", key]) {
        Ok(value) => {
            let values = value
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            anyhow::ensure!(
                values.len() == 1,
                "Repository configuration {key} must have at most one value"
            );
            Ok(values.first().map(|value| (*value).to_owned()))
        }
        Err(_) => {
            runner.remaining()?;
            Ok(None)
        }
    }
}
