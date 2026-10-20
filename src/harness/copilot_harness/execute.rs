use std::time::{Duration, Instant};

use super::{COPILOT_MODEL_ENV, CopilotHarness};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{AiHarness, HarnessOutcome, LiveProgress, ModelCallUsage, PlanningRequest};

mod input;
mod output;
mod usage;
pub(crate) use input::command;
pub(in crate::harness) use input::prompt_input;
pub(super) use output::{needs_attention, normalize_output, stderr_tail};
pub(super) use usage::UsageFile;
#[cfg(test)]
pub(super) use usage::parse_usage;

#[derive(Debug)]
pub(super) struct ProcessOutput {
    pub(super) stdout: Vec<String>,
    pub(super) stderr: Vec<String>,
    pub(super) success: Option<bool>,
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
        let reasoning_effort =
            crate::harness::validate_reasoning_level("copilot", &request.reasoning_level)?;
        crate::harness::require_application_implementation_boundary("Copilot CLI", request)?;
        let boundary = crate::harness::execution_security::ApplicationBoundary::new(request)
            .map_err(|error| {
                AppError::Other(format!("Cannot start Kool.ad/e sandbox: {error:#}"))
            })?;
        let binary = Self::locate_binary()?.canonicalize().map_err(|error| {
            AppError::Other(format!(
                "Cannot resolve GitHub Copilot CLI executable: {error}"
            ))
        })?;
        let version = checked_binary(&binary)?;
        let model = model.map(str::to_owned).or_else(configured_model);
        let usage_file = supports_usage_output(&binary).then(UsageFile::new);
        let mut argv = command(
            &binary,
            request,
            model.as_deref(),
            usage_file.as_ref().map(|file| file.path.as_path()),
        );
        argv.extend(["--effort".into(), reasoning_effort.into()]);
        let mut child_env = Vec::new();
        boundary
            .configure(
                crate::harness::execution_security::CliProvider::Copilot,
                &mut argv,
                &mut child_env,
            )
            .map_err(|error| {
                AppError::Other(format!("Cannot configure Copilot sandbox tools: {error:#}"))
            })?;
        let provider = crate::harness::execution_security::CliProvider::Copilot;
        let evidence = crate::harness::execution_security::ProfileProbeEvidence::probe(
            provider, &binary, version,
        )
        .map_err(AppError::from_provider_profile_error)?;
        let launch =
            crate::harness::execution_security::ValidatedProviderLaunch::bind_for_workspace(
                evidence,
                argv,
                boundary.working_directory(),
                child_env,
                &request.repo_root,
            )
            .map_err(|error| {
                AppError::Other(format!("Copilot launch profile is invalid: {error}"))
            })?;
        let launch_profile = launch.telemetry(
            request
                .prompt_manifest
                .as_ref()
                .map(|manifest| manifest.role_version.as_str()),
        );
        let input = prompt_input(request, boundary.system_policy());
        let started = chrono::Utc::now();
        let task = launch.spawn_with_input(boundary.working_directory(), Some(input))?;
        let output = run_task(task, request)?;
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
            telemetry: crate::harness::ActivityTelemetry {
                launch_profile: Some(launch_profile),
                ..Default::default()
            },
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

#[cfg(test)]
pub(super) fn run_process(
    request: &PlanningRequest,
    argv: &[String],
    input: String,
) -> Result<ProcessOutput, AppError> {
    run_process_at(argv, input, &request.repo_root, request, &[])
}

#[cfg(test)]
fn run_process_at(
    argv: &[String],
    input: String,
    working_directory: &std::path::Path,
    request: &PlanningRequest,
    env: &[(String, String)],
) -> Result<ProcessOutput, AppError> {
    let task = crate::harness::pi_proc::spawn_with_input_provider_env(
        argv,
        working_directory,
        Some(input),
        env,
        crate::harness::execution_security::CliProvider::Copilot,
        &request.repo_root,
    )?;
    run_task(task, request)
}

fn run_task(
    task: crate::harness::pi_proc::ChildTask,
    request: &PlanningRequest,
) -> Result<ProcessOutput, AppError> {
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

pub(super) fn checked_binary(path: &std::path::Path) -> Result<String, AppError> {
    let (output, success) = super::run(path, &["--version"]).map_err(|error| {
        AppError::Other(format!("GitHub Copilot CLI version probe failed: {error}"))
    })?;
    if !success {
        return Err(AppError::Other(
            "GitHub Copilot CLI version command failed".into(),
        ));
    }
    let version = super::copilot_version(&output).ok_or_else(|| {
        AppError::Other(
            "Executable did not identify itself as GitHub Copilot CLI with a version from --version"
                .into(),
        )
    })?;
    crate::harness::execution_security::ProfileProbeEvidence::probe(
        crate::harness::execution_security::CliProvider::Copilot,
        path,
        version.clone(),
    )
    .map_err(|error| AppError::Other(error.to_string()))?;
    Ok(version)
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
