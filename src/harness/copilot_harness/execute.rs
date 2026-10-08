use std::time::{Duration, Instant};

use super::{COPILOT_MODEL_ENV, CopilotHarness};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{AiHarness, HarnessOutcome, LiveProgress, ModelCallUsage, PlanningRequest};

mod usage;
pub(super) use usage::UsageFile;
#[cfg(test)]
pub(super) use usage::parse_usage;

#[derive(Debug)]
pub(super) struct ProcessOutput {
    pub(super) stdout: Vec<String>,
    pub(super) stderr: Vec<String>,
    pub(super) success: Option<bool>,
}

pub(crate) fn command(
    binary: &std::path::Path,
    request: &PlanningRequest,
    model: Option<&str>,
    usage_path: Option<&std::path::Path>,
) -> Vec<String> {
    let mut args = vec![
        binary.to_string_lossy().into_owned(),
        "--silent".into(),
        "--no-ask-user".into(),
        "--no-auto-update".into(),
    ];
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        args.extend(["--model".into(), model.to_owned()]);
    }
    let access = request.mode.tool_access();
    let tools = match access {
        crate::harness::ToolAccess::None => "",
        crate::harness::ToolAccess::ReadOnly => "read",
        crate::harness::ToolAccess::BoundedImplementation => "read,edit",
    };
    args.push(format!("--available-tools={tools}"));
    if access == crate::harness::ToolAccess::BoundedImplementation {
        args.extend([
            "--allow-tool=write".into(),
            "--deny-tool=shell(git:*)".into(),
            "--deny-tool=shell(gh:*)".into(),
        ]);
    }
    args.push("--disable-builtin-mcps".into());
    if let Some(usage_path) = usage_path {
        args.extend([
            "--usage-output-file".into(),
            usage_path.to_string_lossy().into_owned(),
        ]);
    }
    let secrets = std::env::vars_os()
        .filter_map(|(name, _)| {
            let name = name.to_string_lossy();
            let upper = name.to_ascii_uppercase();
            ["TOKEN", "KEY", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH"]
                .iter()
                .any(|needle| upper.contains(needle))
                .then(|| name.into_owned())
        })
        .collect::<Vec<_>>();
    if !secrets.is_empty() {
        args.push(format!("--secret-env-vars={}", secrets.join(",")));
    }
    args
}

impl AiHarness for CopilotHarness {
    fn label(&self) -> String {
        "GitHub Copilot CLI".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        let report = Self::probe_report();
        match (report.readiness, report.version) {
            (super::CopilotReadiness::Ready, Some(version)) => Ok(version),
            (super::CopilotReadiness::Missing, _) => Err(AppError::HarnessNotFound {
                detail: report.diagnostic,
            }),
            _ => Err(AppError::Other(report.diagnostic)),
        }
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        self.execute_with_model(request, None)
    }
    fn execute_with_model(
        &self,
        request: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<HarnessOutcome, AppError> {
        let binary = Self::locate_binary()?.canonicalize().map_err(|error| {
            AppError::Other(format!(
                "Cannot resolve GitHub Copilot CLI executable: {error}"
            ))
        })?;
        let model = model.map(str::to_owned).or_else(configured_model);
        let usage_file = supports_usage_output(&binary).then(UsageFile::new);
        let argv = command(
            &binary,
            request,
            model.as_deref(),
            usage_file.as_ref().map(|file| file.path.as_path()),
        );
        let mut developer = String::from(
            "Kool.ad/e owns task lifecycle, branches, approvals, Git operations, and pull request publication. Never switch branches, commit, push, open a pull request, or publish. Work only inside the current task repository and return the requested structured result.\n\n",
        );
        developer.push_str(&request.system_instructions);
        let input = format!("{developer}\n\n{}", request.prompt_body);
        let started = chrono::Utc::now();
        let output = run_process(request, &argv, input)?;
        let response = normalize_output(&output.stdout);
        if output.success != Some(true) {
            let diagnostics = output
                .stderr
                .iter()
                .chain(output.stdout.iter())
                .collect::<Vec<_>>();
            if needs_attention(&diagnostics) {
                return Err(AppError::Other("GitHub Copilot CLI needs attention; check its authentication and configuration".into()));
            }
            return Err(AppError::HarnessFailed {
                reason: "GitHub Copilot CLI exited with a failure status".into(),
                stderr_tail: stderr_tail(&output.stderr),
            });
        }
        let response = response.ok_or_else(|| AppError::HarnessFailed {
            reason: "GitHub Copilot CLI completed without a final result".into(),
            stderr_tail: stderr_tail(&output.stderr),
        })?;
        let usage = usage_file.as_ref().map(UsageFile::read).unwrap_or_default();
        let now = chrono::Utc::now();
        let _ = request.progress_tx.send(LiveProgress {
            response: response.clone(),
            model_calls: vec![ModelCallUsage {
                call_id: "copilot:call-0".into(),
                provider: Some("github-copilot".into()),
                model: usage.model.clone().or_else(|| model.clone()),
                requested_model: model,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                started_at: Some(started),
                ended_at: Some(now),
                stop_reason: Some("completed".into()),
                ..Default::default()
            }],
            ..Default::default()
        });
        Ok(HarnessOutcome {
            final_text: response,
            envelope: None,
            stderr_tail: stderr_tail(&output.stderr),
        })
    }
}

pub(super) fn run_process(
    request: &PlanningRequest,
    argv: &[String],
    input: String,
) -> Result<ProcessOutput, AppError> {
    let task = crate::harness::pi_proc::spawn_with_input(argv, &request.repo_root, Some(input))?;
    let deadline = Instant::now() + request.timeout;
    let mut stderr = Vec::new();
    let mut stdout = Vec::new();
    let mut status = None;
    loop {
        if request.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            task.kill();
            let _ = task.settle(Duration::from_secs(3));
            return Err(AppError::HarnessFailed {
                reason: "cancelled by user".into(),
                stderr_tail: stderr_tail(&stderr),
            });
        }
        if Instant::now() >= deadline {
            task.kill();
            let _ = task.settle(Duration::from_secs(3));
            return Err(AppError::HarnessTimedOut {
                secs: request.timeout.as_secs(),
            });
        }
        match task.poll_next(Duration::from_millis(200)) {
            Ok(StreamEvt::Stdout(line)) => {
                stdout.push(line);
                let _ = request.progress_tx.send(LiveProgress {
                    response: stdout.join("\n"),
                    ..Default::default()
                });
            }
            Ok(StreamEvt::Stderr(line)) => {
                stderr.push(line);
                if stderr.len() > 100 {
                    stderr.remove(0);
                }
            }
            Ok(StreamEvt::Exited(success)) => status = Some(success),
            Err(PollState::Closed) => break,
            Err(PollState::Pending) => {}
        }
    }
    Ok(ProcessOutput {
        stdout,
        stderr,
        success: status,
    })
}

fn configured_model() -> Option<String> {
    std::env::var(COPILOT_MODEL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            let home = std::env::var_os("COPILOT_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|home| std::path::PathBuf::from(home).join(".copilot"))
                })?;
            std::fs::read(home.join("settings.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|settings| {
                    settings
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .filter(|value| !value.trim().is_empty())
        })
}

pub(super) fn supports_usage_output(binary: &std::path::Path) -> bool {
    super::run(binary, &["--help"])
        .is_ok_and(|(output, success)| success && output.contains("--usage-output-file"))
}

pub(super) fn normalize_output(lines: &[String]) -> Option<String> {
    let text = lines.join("\n").trim().to_owned();
    (!text.is_empty()).then_some(text)
}

pub(super) fn needs_attention(lines: &[&String]) -> bool {
    lines.iter().any(|line| {
        let line = line.to_ascii_lowercase();
        ["auth", "login", "config", "policy", "copilot access"]
            .iter()
            .any(|needle| line.contains(needle))
    })
}

fn stderr_tail(lines: &[String]) -> String {
    const LIMIT: usize = 16_000;
    let joined = lines.join("\n");
    if joined.chars().count() <= LIMIT {
        return joined;
    }
    joined
        .chars()
        .rev()
        .take(LIMIT)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
