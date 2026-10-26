use std::{path::Path, time::Duration};

use crate::execution::contract::ReadinessState;

const AUTH_TIMEOUT: Duration = Duration::from_secs(8);
const AUTH_OUTPUT_LIMIT: usize = 16 * 1024;

/// Inspect only whether OpenCode reports configured credentials. Stored credentials do not prove
/// that the account or provider currently accepts requests, so any non-empty result stays unknown.
pub(super) fn probe(binary: &Path) -> ReadinessState {
    let argv = [
        binary.to_string_lossy().into_owned(),
        "--pure".into(),
        "auth".into(),
        "list".into(),
    ];
    let output = match crate::harness::discovery::run(
        &argv,
        AUTH_TIMEOUT,
        AUTH_OUTPUT_LIMIT,
        AUTH_OUTPUT_LIMIT,
        crate::harness::execution_security::CliProvider::OpenCode.excluded_child_environment(),
    ) {
        Ok(output) if output.success && !output.stdout_truncated && !output.stderr_truncated => {
            output.stdout
        }
        Ok(output) if output.stdout_truncated || output.stderr_truncated => {
            return ReadinessState::Unknown;
        }
        Ok(output) if !output.success => return ReadinessState::Unknown,
        Err(_) => return ReadinessState::Unknown,
        Ok(_) => return ReadinessState::Unknown,
    };
    parse_auth_list(&output)
}

fn parse_auth_list(output: &[u8]) -> ReadinessState {
    let Ok(output) = std::str::from_utf8(output) else {
        return ReadinessState::Unknown;
    };
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        Credentials,
        Environment,
    }
    let mut section = None;
    let mut credential_count = None;
    let mut environment_count = None;
    let mut credential_rows = 0_u64;
    let mut environment_rows = 0_u64;
    for line in output.lines() {
        let body = line_body(line);
        if body.is_empty() {
            continue;
        }
        if let Some(path) = body.strip_prefix("Credentials ") {
            if section.is_some() || path.trim().is_empty() || credential_count.is_some() {
                return ReadinessState::Unknown;
            }
            section = Some(Section::Credentials);
            continue;
        }
        if body == "Environment" {
            if section != Some(Section::Credentials)
                || credential_count.is_none()
                || environment_count.is_some()
            {
                return ReadinessState::Unknown;
            }
            section = Some(Section::Environment);
            continue;
        }
        if let Some(count) = summary_count(body, "credential", "credentials") {
            if section != Some(Section::Credentials) || credential_count.replace(count).is_some() {
                return ReadinessState::Unknown;
            }
            continue;
        }
        if let Some(count) = summary_count(body, "environment variable", "environment variables") {
            if section != Some(Section::Environment) || environment_count.replace(count).is_some() {
                return ReadinessState::Unknown;
            }
            continue;
        }
        if body
            .strip_prefix("● ")
            .is_some_and(|label| !label.trim().is_empty())
        {
            let row_count = match section {
                Some(Section::Credentials) if credential_count.is_none() => &mut credential_rows,
                Some(Section::Environment) if environment_count.is_none() => &mut environment_rows,
                _ => return ReadinessState::Unknown,
            };
            let Some(next_count) = row_count.checked_add(1) else {
                return ReadinessState::Unknown;
            };
            *row_count = next_count;
            continue;
        }
        return ReadinessState::Unknown;
    }
    match (section, credential_count, environment_count) {
        (Some(Section::Environment), Some(credentials), Some(environment))
            if credentials == credential_rows
                && environment == environment_rows
                && (credentials > 0 || environment > 0) =>
        {
            ReadinessState::Unknown
        }
        (Some(Section::Environment), Some(0), Some(0))
            if credential_rows == 0 && environment_rows == 0 =>
        {
            ReadinessState::Unavailable
        }
        _ => ReadinessState::Unknown,
    }
}

fn summary_count(line: &str, singular: &str, plural: &str) -> Option<u64> {
    let line = line.trim();
    let mut fields = line.split_whitespace();
    let count = fields.next()?.parse().ok()?;
    let label = fields.collect::<Vec<_>>().join(" ");
    let expected = if count == 1 { singular } else { plural };
    (label == expected).then_some(count)
}

fn line_body(line: &str) -> &str {
    line.trim().trim_start_matches(['┌', '└', '├', '│']).trim()
}

#[cfg(test)]
#[path = "opencode_authentication/tests.rs"]
mod tests;
