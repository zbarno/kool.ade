//! Unified application error type. Every failure the UI surfaces flows through
//! [`AppError`] so callers can display a single, user-meaningful string.

/// Errors produced by the planner core. Kept deliberately coarse-grained for
/// the MVP: enough to distinguish recoverable situations (shown as banners)
/// from hard failures, never carrying secrets or raw model output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// Repository path does not exist or is not a git working tree.
    InvalidRepo { path: String, detail: String },
    /// A planning artifact could not be read or parsed.
    Artifact { path: String, detail: String },
    /// Git operation failed (missing binary, hooks, identity, etc.).
    Git { cmd: String, detail: String },
    /// The external harness (pi) could not be located.
    HarnessNotFound { detail: String },
    /// The external harness ran but did not produce a usable result.
    HarnessFailed { reason: String, stderr_tail: String },
    /// The harness exceeded its wall-clock budget.
    HarnessTimedOut { secs: u64 },
    /// The structured response failed validation; no files were mutated.
    InvalidResponse { problems: Vec<String> },
    /// Generic filesystem or IO failure.
    Io { op: String, detail: String },
    /// Catch-all used sparingly (wraps anyhow from deep internals).
    Other(String),
}

impl AppError {
    /// One-line, user-facing summary (suitable for a dialog or banner).
    pub fn headline(&self) -> String {
        match self {
            Self::InvalidRepo { path, .. } => {
                format!("Not a valid git repository: {}", redact_secrets(path))
            }
            Self::Artifact { path, .. } => {
                format!("Could not use {}", redact_secrets(path))
            }
            Self::Git { cmd, .. } => format!("git {} failed", redact_secrets(cmd)),
            Self::HarnessNotFound { .. } => "Pi harness not found".to_string(),
            Self::HarnessFailed {
                reason,
                stderr_tail,
            } => {
                let tail = tail_snippet(stderr_tail);
                if tail.is_empty() {
                    format!("Pi harness failed: {}", redact_secrets(reason))
                } else {
                    format!(
                        "Pi harness failed: {} (pi: {})",
                        redact_secrets(reason),
                        redact_secrets(&tail)
                    )
                }
            }
            Self::HarnessTimedOut { secs } => format!("Pi took longer than {secs}s"),
            Self::InvalidResponse { problems } => {
                format!(
                    "Rejected invalid planning response ({})",
                    redact_secrets(&truncate(
                        problems.first().map(String::as_str).unwrap_or(""),
                        160
                    ))
                )
            }
            Self::Io { op, .. } => format!("IO problem during {}", redact_secrets(op)),
            Self::Other(m) => redact_secrets(m),
        }
    }

    /// Detailed (multi-line) explanation for dialogs/logs.
    pub fn detail(&self) -> String {
        match self {
            Self::InvalidRepo { path, detail } => redact_secrets(&format!("{path}: {detail}")),
            Self::Artifact { path, detail } => redact_secrets(&format!("{path}: {detail}")),
            Self::Git { cmd: _, detail } => redact_secrets(detail),
            Self::HarnessNotFound { detail } => redact_secrets(detail),
            Self::HarnessFailed {
                reason,
                stderr_tail,
            } => redact_secrets(&format!("{reason}\nstderr:\n{}", stderr_tail.trim())),
            Self::HarnessTimedOut { secs } => {
                format!(
                    "The configured planning budget expired after {secs}s. Already saved stories are preserved; retry task generation to resume. Set KOOLADE_TURN_TIMEOUT_SECS before starting Kool.ad/e to change the budget."
                )
            }
            Self::InvalidResponse { problems } => redact_secrets(&problems.join("\n")),
            Self::Io { op, detail } => redact_secrets(&format!("{op}: {detail}")),
            Self::Other(m) => redact_secrets(m),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} — {}", self.headline(), self.detail())
    }
}

impl std::error::Error for AppError {}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(redact_secrets(&e.to_string()))
    }
}

/// Remove URL userinfo before command output or diagnostics reach the UI or
/// durable task evidence. Keeps the protocol and host/path for useful context.
pub fn redact_secrets(input: &str) -> String {
    const SCHEMES: [&str; 4] = ["https://", "http://", "ssh://", "git://"];
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    loop {
        let next = SCHEMES
            .iter()
            .filter_map(|scheme| {
                input.as_bytes()[cursor..]
                    .windows(scheme.len())
                    .position(|window| window.eq_ignore_ascii_case(scheme.as_bytes()))
                    .map(|relative| (cursor + relative, scheme.len()))
            })
            .min_by_key(|(start, _)| *start);
        let Some((start, scheme_len)) = next else {
            output.push_str(&input[cursor..]);
            break;
        };
        let authority_start = start + scheme_len;
        output.push_str(&input[cursor..authority_start]);
        let authority_end = input.as_bytes()[authority_start..]
            .iter()
            .position(|byte| {
                byte.is_ascii_whitespace()
                    || matches!(
                        *byte,
                        b'/' | b'\\' | b'?' | b'#' | b'"' | b'\'' | b',' | b')' | b']' | b'}'
                    )
            })
            .map(|relative| authority_start + relative)
            .unwrap_or(input.len());
        let authority = &input[authority_start..authority_end];
        if let Some((_, host)) = authority.rsplit_once('@') {
            output.push_str("[REDACTED]@");
            output.push_str(host);
        } else {
            output.push_str(authority);
        }
        cursor = authority_end;
    }
    output
}

/// Prefer the actual error over a runtime version footer or stack frame.
fn tail_snippet(t: &str) -> String {
    let lines: Vec<_> = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines
        .iter()
        .find(|l| l.contains("ENOSPC") || l.contains("no space left on device"))
        .or_else(|| {
            lines.iter().find(|l| {
                l.contains("Error:") || l.contains("Error [") || l.contains("FATAL ERROR")
            })
        })
        .or_else(|| {
            lines.iter().rev().find(|l| {
                !l.starts_with("Node.js v") && !l.starts_with("at ") && !matches!(**l, "}" | "^")
            })
        })
        .map(|l| truncate(l, 240))
        .unwrap_or_default()
}

/// Keep short strings tidy inside headlines.
fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_full_error_is_not_hidden_by_node_footer() {
        let error = AppError::HarnessFailed { reason: "pi exited with a failure code".into(), stderr_tail: "Error: ENOSPC: no space left on device, write\n    at emitErrorNT (node:internal/streams/destroy:170:8)\n}\nNode.js v22.23.2".into() };
        assert!(error.headline().contains("ENOSPC"));
        assert!(!error.headline().contains("v22.23.2"));
        assert!(error.detail().contains("emitErrorNT"));
    }

    #[test]
    fn userinfo_is_removed_from_urls_in_diagnostics() {
        let input = "fatal: unable to access 'https://alice:ghp_secret@github.com/acme/repo.git': denied; ssh://build:password@git.example/a/b; HTTPS://synthetic-user:synthetic-pass@packages.example.net/pkg";
        let redacted = redact_secrets(input);
        assert_eq!(
            redacted,
            "fatal: unable to access 'https://[REDACTED]@github.com/acme/repo.git': denied; ssh://[REDACTED]@git.example/a/b; HTTPS://[REDACTED]@packages.example.net/pkg"
        );
        assert!(!redacted.contains("ghp_secret"));
        assert!(!redacted.contains("password"));
        assert!(!redacted.contains("synthetic-pass"));
    }
}
