use std::time::{Duration, Instant};

use super::{CODEX_MODEL_ENV, CodexHarness, STDERR_LIMIT};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{
    AiHarness, HarnessOutcome, LivePost, LiveProgress, ModelCallUsage, PlanningRequest,
};

mod failures;
mod helpers;
use failures::{is_host_code_mode_disabled, provider_failure};
#[cfg(test)]
pub(crate) use helpers::normalize_effort;
use helpers::stderr_tail;
pub(crate) use helpers::{CodexEvent, command, parse_event};

impl AiHarness for CodexHarness {
    fn label(&self) -> String {
        match self.check_available() {
            Ok(version) => format!("codex {version}"),
            Err(AppError::HarnessNotFound { .. }) => "codex (not installed)".into(),
            Err(error) => format!("codex (unavailable: {})", error.headline()),
        }
    }

    fn check_available(&self) -> Result<String, AppError> {
        Self::check_binary(&Self::locate_binary()?)
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        self.execute_with_model(request, None)
    }

    fn execute_with_model(
        &self,
        request: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<HarnessOutcome, AppError> {
        crate::harness::validate_reasoning_level("codex", &request.reasoning_level)?;
        crate::harness::require_application_implementation_boundary("Codex", request)?;
        let boundary = crate::harness::execution_security::ApplicationBoundary::new(request)
            .map_err(|error| {
                AppError::Other(format!("Cannot start Kool.ad/e sandbox: {error:#}"))
            })?;
        let binary = Self::locate_binary()?.canonicalize().map_err(|error| {
            AppError::Other(format!("Cannot resolve Codex CLI executable: {error}"))
        })?;
        let version = Self::check_binary(&binary)?;
        let requested_model = model.map(Into::into).or_else(|| {
            std::env::var(CODEX_MODEL_ENV)
                .ok()
                .filter(|model| !model.trim().is_empty())
                .map(Into::into)
        });
        let requested_model_name = requested_model
            .as_ref()
            .map(|model: &std::ffi::OsString| model.to_string_lossy().into_owned());
        let (mut argv, prompt) = command(
            &binary,
            request,
            requested_model,
            boundary.working_directory(),
        );
        let mut child_env = Vec::new();
        boundary
            .configure(
                crate::harness::execution_security::CliProvider::Codex,
                &mut argv,
                &mut child_env,
            )
            .map_err(|error| {
                AppError::Other(format!("Cannot configure Codex sandbox tools: {error:#}"))
            })?;
        boundary.set_telemetry_model(requested_model_name.as_deref());
        let provider = crate::harness::execution_security::CliProvider::Codex;
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
                AppError::Other(format!("Codex launch profile is invalid: {error}"))
            })?;
        let launch_profile = launch.telemetry(
            request
                .prompt_manifest
                .as_ref()
                .map(|manifest| manifest.role_version.as_str()),
        );
        let prompt = format!("{prompt}\n\n{}", boundary.system_policy());
        let task = launch.spawn_with_input(boundary.working_directory(), Some(prompt))?;
        let deadline = Instant::now() + request.timeout;
        let mut stderr = Vec::new();
        let mut exit_status = None;
        let mut final_text = String::new();
        let mut deferred_host_code_mode_failure = None;
        let mut activity = Vec::new();
        let mut model_calls = Vec::new();
        let started_at = chrono::Utc::now();
        let started = Instant::now();
        let mut telemetry = crate::harness::ActivityTelemetry {
            started_ms: Some(started_at.timestamp_millis()),
            launch_profile: Some(launch_profile),
            ..Default::default()
        };

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
                    telemetry.updated_ms = Some(chrono::Utc::now().timestamp_millis());
                    telemetry.updates += 1;
                    if let Some(event) = parse_event(&line) {
                        match event {
                            CodexEvent::Message(text) => {
                                final_text = text;
                                deferred_host_code_mode_failure = None;
                            }
                            CodexEvent::Activity(text) => activity.push(text),
                            CodexEvent::Usage {
                                call_id,
                                input,
                                output,
                                cached_input,
                                cache_write_input,
                            } => {
                                let end = chrono::Utc::now();
                                model_calls.push(ModelCallUsage {
                                    call_id: call_id.unwrap_or_else(|| {
                                        format!("codex:turn-{}", model_calls.len())
                                    }),
                                    provider: Some("openai".into()),
                                    api: Some("codex-cli-turn-aggregate".into()),
                                    model: None,
                                    requested_model: requested_model_name.clone(),
                                    input_tokens: input,
                                    output_tokens: output,
                                    cache_read_tokens: cached_input,
                                    cache_write_tokens: cache_write_input,
                                    total_tokens: input.zip(output).map(|(i, o)| i + o),
                                    started_at: Some(started_at),
                                    ended_at: Some(end),
                                    duration_millis: Some(started.elapsed().as_millis() as u64),
                                    stop_reason: Some("completed".into()),
                                    ..Default::default()
                                });
                            }
                            CodexEvent::Failure(reason) => {
                                if is_host_code_mode_disabled(&reason) {
                                    // Codex can report its deliberately disabled
                                    // host-native path while continuing with the
                                    // application MCP tools. Defer the warning
                                    // until the turn ends; a later assistant
                                    // result proves the warning was recoverable.
                                    deferred_host_code_mode_failure = Some(reason);
                                    continue;
                                }
                                return Err(provider_failure(
                                    &reason,
                                    request.mode.tool_access(),
                                    stderr_tail(&stderr),
                                ));
                            }
                            CodexEvent::Other => {}
                        }
                        let progress = LiveProgress {
                            telemetry: telemetry.clone(),
                            model_calls: model_calls.clone(),
                            posts: activity
                                .iter()
                                .enumerate()
                                .map(|(index, text)| LivePost {
                                    id: (0, index),
                                    kind: "activity".into(),
                                    text: text.clone(),
                                })
                                .collect(),
                            activity: activity.last().cloned(),
                            response: final_text.clone(),
                            ..LiveProgress::default()
                        };
                        let _ = request.progress_tx.send(progress);
                    }
                }
                Ok(StreamEvt::Stderr(line)) => {
                    stderr.push(line);
                    if stderr.len() > STDERR_LIMIT {
                        stderr.remove(0);
                    }
                }
                Ok(StreamEvt::Exited(success)) => exit_status = Some(success),
                Err(PollState::Closed) => break,
                Err(PollState::Pending) => {}
            }
        }
        if let Some(reason) = deferred_host_code_mode_failure {
            return Err(provider_failure(
                &reason,
                request.mode.tool_access(),
                stderr_tail(&stderr),
            ));
        }
        match exit_status {
            Some(true) => {}
            Some(false) => {
                return Err(AppError::HarnessFailed {
                    reason: "Codex CLI exited with a failure status".into(),
                    stderr_tail: stderr_tail(&stderr),
                });
            }
            None => {
                return Err(AppError::HarnessFailed {
                    reason: "Codex CLI output stream ended before process completion".into(),
                    stderr_tail: stderr_tail(&stderr),
                });
            }
        }
        if final_text.trim().is_empty() {
            return Err(AppError::HarnessFailed {
                reason: "Codex CLI completed without a final assistant message".into(),
                stderr_tail: stderr_tail(&stderr),
            });
        }
        telemetry.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        let _ = request.progress_tx.send(LiveProgress {
            telemetry,
            model_calls,
            posts: activity
                .iter()
                .enumerate()
                .map(|(index, text)| LivePost {
                    id: (0, index),
                    kind: "activity".into(),
                    text: text.clone(),
                })
                .collect(),
            activity: activity.last().cloned(),
            response: final_text.clone(),
            ..LiveProgress::default()
        });
        Ok(HarnessOutcome {
            final_text,
            envelope: None,
            stderr_tail: stderr_tail(&stderr),
        })
    }
}
