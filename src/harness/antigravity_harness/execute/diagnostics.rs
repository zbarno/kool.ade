use crate::error::AppError;

pub(in crate::harness::antigravity_harness) fn execution_error(
    exit: Option<bool>,
    stderr: &[String],
    unexpected_stdout: &[String],
) -> AppError {
    let joined = stderr.join("\n");
    let lower = joined.to_ascii_lowercase();
    if let Some(attention) = provider_attention(&lower) {
        return AppError::Other(attention);
    }
    AppError::HarnessFailed {
        reason: if exit == Some(false) {
            "Antigravity exited with a failure status".into()
        } else {
            "Antigravity output stream ended before a result".into()
        },
        stderr_tail: diagnostics_tail(stderr, unexpected_stdout),
    }
}

pub(super) fn record_unexpected(lines: &mut Vec<String>, line: &str) {
    const MAX_LINES: usize = 8;
    const MAX_CHARS: usize = 512;
    if lines.len() == MAX_LINES {
        lines.remove(0);
    }
    let safe = redact_unexpected(line);
    lines.push(safe.chars().take(MAX_CHARS).collect());
}

pub(in crate::harness::antigravity_harness) fn redact_unexpected(line: &str) -> String {
    let mut output = crate::error::redact_secrets(line);
    for (name, value) in std::env::vars_os() {
        let name = name.to_string_lossy().to_ascii_uppercase();
        if ["TOKEN", "KEY", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH"]
            .iter()
            .any(|part| name.contains(part))
        {
            let value = value.to_string_lossy();
            if !value.is_empty() {
                output = output.replace(value.as_ref(), "[REDACTED]");
            }
        }
    }
    redact_labeled_values(&output)
}

fn redact_labeled_values(input: &str) -> String {
    const LABELS: [&str; 13] = [
        "bearer",
        "api key",
        "api_key",
        "api-key",
        "apikey",
        "access key",
        "access_token",
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
        "gemini_api_key",
    ];
    let mut output = input.to_owned();
    let mut cursor = 0;
    loop {
        let lower = output.to_ascii_lowercase();
        let next = LABELS
            .iter()
            .filter_map(|label| {
                lower[cursor..]
                    .find(label)
                    .map(|offset| (cursor + offset, label.len()))
            })
            .min_by_key(|(index, _)| *index);
        let Some((label_start, label_len)) = next else {
            break;
        };
        let mut value_start = label_start + label_len;
        while value_start < output.len()
            && matches!(
                output.as_bytes()[value_start],
                b' ' | b'\t' | b'=' | b':' | b'"' | b'\''
            )
        {
            value_start += 1;
        }
        let value_end = output[value_start..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | ';' | '}'))
            .map(|offset| value_start + offset)
            .unwrap_or(output.len());
        if value_start < value_end {
            output.replace_range(value_start..value_end, "[REDACTED]");
            cursor = value_start + "[REDACTED]".len();
        } else {
            cursor = label_start + label_len;
        }
        if cursor >= output.len() {
            break;
        }
    }
    output
}

pub(super) fn diagnostics_tail(stderr: &[String], unexpected_stdout: &[String]) -> String {
    let mut details = stderr_tail(stderr);
    if !unexpected_stdout.is_empty() {
        if !details.is_empty() {
            details.push('\n');
        }
        details.push_str("Unrecognized Antigravity stdout:\n");
        details.push_str(&unexpected_stdout.join("\n"));
    }
    details
}

pub(in crate::harness::antigravity_harness) fn provider_attention(detail: &str) -> Option<String> {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("authentication required")
        || lower.contains("not authenticated")
        || lower.contains("no credentials")
        || lower.contains("api key")
    {
        Some("Antigravity needs attention: authenticate with `agy` in an interactive terminal, then retry.".into())
    } else if lower.contains("modelprovider")
        || lower.contains("provider configuration")
        || lower.contains("model provider")
    {
        Some("Antigravity needs attention: configure a model provider in Antigravity settings, then retry.".into())
    } else {
        None
    }
}
pub(in crate::harness::antigravity_harness) fn looks_like_denial(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("soft-denied")
        || line.contains("permission denied")
        || line.contains("requires approval")
        || line.contains("could not obtain approval")
}
pub(in crate::harness::antigravity_harness) fn stderr_tail(lines: &[String]) -> String {
    lines
        .iter()
        .rev()
        .take(12)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| redact_unexpected(&line))
        .collect::<Vec<_>>()
        .join("\n")
}
