use std::{path::Path, time::Duration};

use crate::error::AppError;

use super::{CodexHarness, CodexProbeReport, CodexReadiness};

const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Ceiling for one bounded live model-access verification round trip.
const LIVE_VERIFY_TIMEOUT: Duration = Duration::from_secs(60);
/// Captured output ceiling for the live verification subprocess.
const LIVE_VERIFY_OUTPUT_LIMIT: usize = 1024 * 1024;
/// Fixed, extremely small prompt for the verification round trip.
const LIVE_VERIFY_PROMPT: &str = "Kool.ad/e readiness handshake. Reply with exactly: ok";
/// Bound for verifier output excerpts kept in diagnostics.
const EVIDENCE_SNIPPET_LIMIT: usize = 160;
/// Persistence key for this provider's detection entry.
const PROVIDER_ID: &str = "codex";

/// Whether a probe may initiate the bounded live verification call. Routine
/// discovery may; opportunistic capability checks refuse and honor only a
/// stored verification receipt, so they stay purely local.
#[derive(Clone, Copy)]
enum VerificationPolicy {
    TrustStoredReceipt,
    AllowLiveCall,
}

impl CodexHarness {
    pub fn probe_report() -> CodexProbeReport {
        let path = match Self::locate_binary() {
            Ok(path) => path,
            Err(error) => {
                return CodexProbeReport {
                    status: "codex (not installed)".into(),
                    version: None,
                    diagnostic: error.detail(),
                    binary: None,
                    ready: false,
                    readiness: CodexReadiness::Missing,
                    verified_at_unix_ms: None,
                };
            }
        };
        let version = run_probe(&path, &["--version"]).ok().and_then(|output| {
            crate::harness::discovery::normalize_cli_version(&output, &["codex-cli"], false)
        });
        match check_binary_readiness(&path, VerificationPolicy::AllowLiveCall) {
            Ok((version, resolved)) => {
                let (status, diagnostic, ready) = match resolved.readiness {
                    CodexReadiness::Ready => (format!("codex {version}"), String::new(), true),
                    CodexReadiness::AuthenticationRequired => (
                        "codex (authentication required)".into(),
                        "Codex CLI is installed but codex login is not completed. Run `codex login` and retry."
                            .into(),
                        false,
                    ),
                    CodexReadiness::AuthenticationUnverified => (
                        "codex (authentication unverified)".into(),
                        format!(
                            "Codex account and model access remain unverified; {}.",
                            resolved
                                .verification_note
                                .clone()
                                .unwrap_or_else(|| {
                                    "a bounded live verification did not complete successfully"
                                        .into()
                                })
                        ),
                        false,
                    ),
                    CodexReadiness::Missing | CodexReadiness::Unusable => (
                        "codex (installed but unusable)".into(),
                        "Codex CLI readiness could not be verified.".into(),
                        false,
                    ),
                };
                CodexProbeReport {
                    status,
                    version: Some(version),
                    diagnostic,
                    binary: Some(path),
                    ready,
                    readiness: resolved.readiness,
                    verified_at_unix_ms: resolved.verified_at_unix_ms,
                }
            }
            Err(error) => CodexProbeReport {
                status: "codex (installed but unusable)".into(),
                version,
                diagnostic: error.detail(),
                binary: Some(path),
                ready: false,
                readiness: CodexReadiness::Unusable,
                verified_at_unix_ms: None,
            },
        }
    }

    pub(super) fn check_binary(path: &Path) -> Result<String, AppError> {
        let (version, resolved) =
            check_binary_readiness(path, VerificationPolicy::TrustStoredReceipt)?;
        match resolved.readiness {
            CodexReadiness::Ready => Ok(version),
            CodexReadiness::AuthenticationRequired => Err(AppError::Other(
                "Codex CLI is installed but codex login is not completed. Run `codex login` and retry."
                    .into(),
            )),
            CodexReadiness::AuthenticationUnverified => {
                // The test-only override exercises fake CLI lifecycle paths; it is
                // not evidence for discovery and is absent from production builds.
                #[cfg(test)]
                if crate::harness::uncontained_provider_test_execution_enabled() {
                    return Ok(version);
                }
                let mut detail =
                    "Codex account and model access remain unverified; execution is blocked.".to_owned();
                if let Some(note) = &resolved.verification_note {
                    detail.push(' ');
                    detail.push_str(note);
                }
                Err(AppError::Other(detail))
            }
            CodexReadiness::Missing | CodexReadiness::Unusable => Err(AppError::Other(
                "Codex CLI readiness could not be verified.".into(),
            )),
        }
    }
}

fn check_binary_readiness(
    path: &Path,
    policy: VerificationPolicy,
) -> Result<(String, ResolvedAccess), AppError> {
    let version = crate::harness::discovery::normalize_cli_version(
        &run_probe(path, &["--version"])?,
        &["codex-cli"],
        false,
    )
    .ok_or_else(|| AppError::Other("Codex CLI did not report a safe version".into()))?;
    #[cfg(test)]
    check_supported_capabilities(path)?;
    #[cfg(not(test))]
    crate::harness::execution_security::ProfileProbeEvidence::probe(
        crate::harness::execution_security::CliProvider::Codex,
        path,
        version.clone(),
    )
    .map_err(|error| AppError::Other(error.to_string()))?;

    // Observe where credentials could exist. A credential store sighting is
    // not proof of account access; resolution below decides whether to trust
    // a stored verification receipt or spend one bounded live check.
    match credentials_observed(path)? {
        None => Ok((
            version,
            ResolvedAccess {
                readiness: CodexReadiness::AuthenticationRequired,
                verified_at_unix_ms: None,
                verification_note: None,
            },
        )),
        Some(evidence) => Ok((version, resolve_verified_access(path, evidence, policy))),
    }
}

struct ResolvedAccess {
    readiness: CodexReadiness,
    verified_at_unix_ms: Option<u64>,
    verification_note: Option<String>,
}

/// Where Codex credentials could plausibly exist. Deliberately carries no
/// output-derived text: diagnostic prose uses fixed wording per source so
/// credential-bearing strings never enter logs or settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialSource {
    ApiKeyEnvironment,
    WorkloadIdentity,
    ApiKeyReportedByCli,
    LoggedInSession,
}

impl CredentialSource {
    fn label(&self) -> &'static str {
        match self {
            Self::ApiKeyEnvironment => "an OpenAI API key was present in the environment",
            Self::WorkloadIdentity => "workload identity selection was present in the environment",
            Self::ApiKeyReportedByCli => "Codex reports an API key configured",
            Self::LoggedInSession => "a logged-in Codex session was observed",
        }
    }
}

/// Classify where Codex credentials could exist, without asserting access.
/// Returns `None` when the CLI says it holds no credentials at all.
fn credentials_observed(path: &Path) -> Result<Option<CredentialSource>, AppError> {
    if api_key_environment_present() {
        return Ok(Some(CredentialSource::ApiKeyEnvironment));
    }
    if workload_identity_selected() {
        return Ok(Some(CredentialSource::WorkloadIdentity));
    }
    let (output, success) = run_probe_status(path, &["login", "status"]).map_err(|_| {
        AppError::Other(
            "Codex CLI login status could not be checked because the command failed.".into(),
        )
    })?;
    let lower = output.to_ascii_lowercase();
    if reports_api_key_configured(&lower) {
        return Ok(Some(CredentialSource::ApiKeyReportedByCli));
    }
    if lower.contains("not logged in")
        || lower.contains("logged out")
        || lower.contains("not authenticated")
    {
        return Ok(None);
    }
    if success {
        // "Logged in as ..." or any other non-negative status: credentials
        // exist; the text itself is deliberately discarded.
        return Ok(Some(CredentialSource::LoggedInSession));
    }
    Err(AppError::Other(
        "Codex CLI login status could not be checked because the command failed.".into(),
    ))
}

fn resolve_verified_access(
    path: &Path,
    evidence: CredentialSource,
    policy: VerificationPolicy,
) -> ResolvedAccess {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let (settings, _) = crate::persistence::harness_settings::load();
    if let Some(entry) = settings.discovered.get(PROVIDER_ID)
        && entry.verification_is_current_at(now)
        && let Some(stamp) = entry.verified_at_unix_ms
    {
        return ResolvedAccess {
            readiness: CodexReadiness::Ready,
            verified_at_unix_ms: Some(stamp),
            verification_note: None,
        };
    }
    match policy {
        VerificationPolicy::TrustStoredReceipt => ResolvedAccess {
            readiness: CodexReadiness::AuthenticationUnverified,
            verified_at_unix_ms: None,
            verification_note: Some("No generative model call was made in this check".into()),
        },
        VerificationPolicy::AllowLiveCall => match run_live_verification(path) {
            Ok(()) => ResolvedAccess {
                readiness: CodexReadiness::Ready,
                verified_at_unix_ms: Some(crate::persistence::harness_settings::current_unix_ms()),
                verification_note: None,
            },
            Err(detail) => ResolvedAccess {
                readiness: CodexReadiness::AuthenticationUnverified,
                verified_at_unix_ms: None,
                verification_note: Some(format!(
                    "live verification failed: {}; observed: {}",
                    detail,
                    evidence.label()
                )),
            },
        },
    }
}

/// One bounded, consent-implied live round trip proving the signed-in account
/// can actually obtain a model response. The call inherits the hardened probe
/// launcher: private working directory, sanitized environment, timeout, and
/// output limits. Success requires a clean exit and a non-empty reply.
fn run_live_verification(binary: &Path) -> Result<(), String> {
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    // The probe runs from a private, non-repository directory, so opt out of
    // git repository checks the same way normal Codex execution does.
    for arg in [
        "exec",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--ephemeral",
        LIVE_VERIFY_PROMPT,
    ] {
        argv.push(arg.to_string());
    }
    let output = crate::harness::discovery::run(
        &argv,
        LIVE_VERIFY_TIMEOUT,
        LIVE_VERIFY_OUTPUT_LIMIT,
        LIVE_VERIFY_OUTPUT_LIMIT,
        crate::harness::execution_security::CliProvider::Codex.excluded_child_environment(),
    )
    .map_err(|error| format!("could not launch the verification call: {error}"))?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err("the verification output exceeded the captured size limit".into());
    }
    if !output.success {
        let tail = output_tail_line(&String::from_utf8_lossy(&output.stderr));
        if tail.is_empty() {
            return Err("the verification call failed or timed out".into());
        }
        return Err(format!("the verification call failed: {tail}"));
    }
    let reply = String::from_utf8_lossy(&output.stdout);
    if reply.trim().is_empty() {
        return Err("the verification call produced no model reply".into());
    }
    Ok(())
}

/// Last non-empty line of verifier stderr, masked and length-bounded, so a
/// terse, safe cause survives in diagnostics without copying raw output.
fn output_tail_line(raw: &str) -> String {
    let line = raw
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let mut scoped: String = line.chars().take(EVIDENCE_SNIPPET_LIMIT).collect();
    if line.chars().count() > EVIDENCE_SNIPPET_LIMIT {
        scoped.push('…');
    }
    scrub_sensitive(scoped)
}

/// Defense in depth for newly surfaced verifier output: the shared redactor
/// covers URL userinfo; this additionally masks common API-token shapes on a
/// word boundary so probe diagnostics stay clean even when a CLI echoes one.
pub(super) fn scrub_sensitive(mut input: String) -> String {
    input = crate::error::redact_secrets(&input);
    for prefix in [
        "sk-ant-",
        "sk-proj-",
        "sk-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "github_pat_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "AKIA",
        "ASIA",
        "AIza",
    ] {
        let mut out = String::with_capacity(input.len());
        let mut rest = input.as_str();
        while let Some(pos) = rest.find(prefix) {
            let preceded_boundary = rest[..pos]
                .chars()
                .next_back()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric());
            let after = &rest[pos + prefix.len()..];
            let token_len = after
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_'))
                .count();
            if preceded_boundary && token_len >= 12 {
                out.push_str(&rest[..pos]);
                out.push_str("[REDACTED]");
                rest = &after[token_len..];
            } else {
                out.push_str(&rest[..pos + prefix.len()]);
                rest = after;
            }
        }
        out.push_str(rest);
        input = out;
    }
    input
}

fn workload_identity_selected() -> bool {
    std::env::var_os("OPENAI_FEDERATION_RULE_ID").is_some()
        || std::env::var_os("OPENAI_IDENTITY_TOKEN_FILE").is_some()
}

fn api_key_environment_present() -> bool {
    std::env::var_os(OPENAI_API_KEY_ENV).is_some()
}

fn reports_api_key_configured(output: &str) -> bool {
    output.contains("api key configured")
        || output.contains("logged in using an api key")
        || output.contains("logged in using api key")
}

#[cfg(test)]
fn check_supported_capabilities(path: &Path) -> Result<(), AppError> {
    let argv = [
        path.to_string_lossy().into_owned(),
        "exec".into(),
        "--help".into(),
    ];
    let output = crate::harness::discovery::run_isolated(
        &argv,
        PROBE_TIMEOUT,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::Codex.excluded_child_environment(),
    )
    .map_err(|_| AppError::Other("Codex CLI capability check failed or timed out".into()))?;
    if !output.success || output.stdout_truncated || output.stderr_truncated {
        return Err(AppError::Other(
            "Codex CLI capability check failed or exceeded the output limit".into(),
        ));
    }
    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let required = [
        "--json",
        "--cd",
        "--sandbox",
        "--ephemeral",
        "--model",
        "--config",
        "--disable",
        "--ignore-user-config",
        "--skip-git-repo-check",
    ];
    let missing = required
        .into_iter()
        .filter(|option| !help.contains(option))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Codex CLI is too old or lacks required exec options: {}",
            missing.join(", ")
        )))
    }
}

fn run_probe(binary: &Path, args: &[&str]) -> Result<String, AppError> {
    let (output, success) = run_probe_status(binary, args)?;
    if success {
        Ok(output)
    } else {
        Err(AppError::Other("Codex CLI readiness check failed".into()))
    }
}

fn run_probe_status(binary: &Path, args: &[&str]) -> Result<(String, bool), AppError> {
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let output = crate::harness::discovery::run(
        &argv,
        PROBE_TIMEOUT,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::Codex.excluded_child_environment(),
    )
    .map_err(|_| AppError::Other("Codex CLI readiness check failed or timed out".into()))?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err(AppError::Other(
            "Codex CLI readiness output exceeded the size limit".into(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok((format!("{stdout}\n{stderr}"), output.success))
}
