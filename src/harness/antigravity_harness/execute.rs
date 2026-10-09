use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

mod diagnostics;
use diagnostics::{diagnostics_tail, record_unexpected};
pub(super) use diagnostics::{
    execution_error, looks_like_denial, provider_attention, redact_unexpected, stderr_tail,
};

use super::{ANTIGRAVITY_MODEL_ENV, AntigravityHarness};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{
    AiHarness, HarnessOutcome, LivePost, LiveProgress, ModelCallUsage, PlanningRequest,
};

pub(super) fn command(
    binary: &Path,
    request: &PlanningRequest,
    model: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        binary.to_string_lossy().into_owned(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
    ];
    if let Some(model) = model.filter(|m| !m.trim().is_empty()) {
        args.extend(["--model".into(), model.to_owned()]);
    }
    let effort = match request.reasoning_level.to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" | "xhigh" => "high",
        _ => "medium",
    };
    args.extend(["--effort".into(), effort.into()]);
    args
}

#[cfg(test)]
pub(super) fn prompt_input(request: &PlanningRequest) -> String {
    prompt_input_with_policy(request, "")
}

fn prompt_input_with_policy(request: &PlanningRequest, boundary_policy: &str) -> String {
    let prompt = format!(
        "Kool.ad/e owns task lifecycle, source and destination branches, approvals, Git operations, and publication. Never switch branches, commit, push, open a pull request, or publish. Work only on the task requested below and return its required structured result.\n\n{}\n\n{}\n\n{}",
        request.system_instructions, boundary_policy, request.prompt_body
    );
    serde_json::json!({"event":"user","message":{"content":prompt}}).to_string() + "\n"
}

impl AiHarness for AntigravityHarness {
    fn label(&self) -> String {
        match self.check_available() {
            Ok(version) => format!("Antigravity {version}"),
            Err(AppError::HarnessNotFound { .. }) => "Antigravity (not installed)".into(),
            Err(error) => format!("Antigravity (unavailable: {})", error.headline()),
        }
    }
    fn check_available(&self) -> Result<String, AppError> {
        Self::checked_binary(&Self::locate_binary()?)
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        self.execute_with_model(request, None)
    }
    fn execute_with_model(
        &self,
        request: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<HarnessOutcome, AppError> {
        crate::harness::require_application_implementation_boundary("Antigravity", request)?;
        let boundary = crate::harness::execution_security::ApplicationBoundary::new(request)
            .map_err(|error| {
                AppError::Other(format!("Cannot start Kool.ad/e sandbox: {error:#}"))
            })?;
        let binary = Self::locate_binary()?.canonicalize().map_err(|e| {
            AppError::Other(format!("Cannot resolve Antigravity CLI executable: {e}"))
        })?;
        let requested_model = model.map(str::to_owned).or_else(|| {
            std::env::var(ANTIGRAVITY_MODEL_ENV)
                .ok()
                .filter(|m| !m.trim().is_empty())
        });
        let mut argv = command(&binary, request, requested_model.as_deref());
        let mut child_env = Vec::new();
        boundary
            .configure(
                crate::harness::execution_security::CliProvider::Antigravity,
                &mut argv,
                &mut child_env,
            )
            .map_err(|error| {
                AppError::Other(format!(
                    "Cannot configure Antigravity sandbox tools: {error:#}"
                ))
            })?;
        let task = crate::harness::pi_proc::spawn_with_input_env_excluding(
            &argv,
            boundary.working_directory(),
            Some(prompt_input_with_policy(request, boundary.system_policy())),
            &child_env,
            crate::harness::execution_security::CliProvider::Antigravity
                .excluded_child_environment(),
        )?;
        let started_at = chrono::Utc::now();
        let started = Instant::now();
        let deadline = started + request.timeout;
        let mut stderr = Vec::new();
        let mut exit = None;
        let mut result = None;
        let mut posts = Vec::new();
        let mut policy_denial = None;
        let mut unexpected_stdout = Vec::new();
        let mut telemetry = crate::harness::ActivityTelemetry {
            started_ms: Some(started_at.timestamp_millis()),
            ..Default::default()
        };
        let mut model_calls = Vec::new();
        let mut deltas = String::new();
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
                    if let Ok(event) = serde_json::from_str::<Value>(&line) {
                        if !matches!(
                            event["event"].as_str(),
                            Some("init" | "step_update" | "result")
                        ) {
                            record_unexpected(&mut unexpected_stdout, &line);
                        }
                        if event["event"] == "step_update" {
                            let step = &event["step_update"];
                            if step["step_type"] == "agent_response"
                                && let Some(delta) = step["text_delta"].as_str()
                            {
                                deltas.push_str(delta);
                            }
                            if step["step_type"] == "tool" {
                                let name = step["tool_name"].as_str().unwrap_or("tool").to_owned();
                                if step["tool_info"]["error"].is_object() {
                                    let detail = step["tool_info"]["error"]["message"]
                                        .as_str()
                                        .unwrap_or("tool permission was denied");
                                    if looks_like_denial(detail) {
                                        policy_denial = Some(detail.to_owned());
                                    }
                                    posts.push(format!("{name} was denied by Antigravity policy"));
                                } else {
                                    posts.push(name);
                                }
                            }
                        }
                        if event["event"] == "result" {
                            result = Some(event["result"].clone());
                        }
                    } else {
                        record_unexpected(&mut unexpected_stdout, &line);
                    }
                    let _ = request.progress_tx.send(progress(
                        &telemetry,
                        &model_calls,
                        &posts,
                        &deltas,
                    ));
                }
                Ok(StreamEvt::Stderr(line)) => {
                    stderr.push(line);
                    if stderr.len() > 100 {
                        stderr.remove(0);
                    }
                }
                Ok(StreamEvt::Exited(success)) => exit = Some(success),
                Err(PollState::Closed) => break,
                Err(PollState::Pending) => {}
            }
        }
        let Some(envelope) = result else {
            return Err(execution_error(exit, &stderr, &unexpected_stdout));
        };
        let status = envelope["status"].as_str().unwrap_or("INVALID");
        if let Some(detail) = policy_denial {
            return Err(AppError::Other(format!(
                "Antigravity needs attention: {}",
                redact_unexpected(&detail)
            )));
        }
        if let Some(line) = stderr.iter().find(|line| looks_like_denial(line)) {
            return Err(AppError::Other(format!(
                "Antigravity needs attention: {}",
                redact_unexpected(line)
            )));
        }
        if status != "SUCCESS" || exit != Some(true) {
            let reason = envelope["error"].as_str().unwrap_or(status).to_owned();
            if let Some(attention) = provider_attention(&reason) {
                return Err(AppError::Other(attention));
            }
            return Err(AppError::HarnessFailed {
                reason: redact_unexpected(&reason),
                stderr_tail: diagnostics_tail(&stderr, &unexpected_stdout),
            });
        }
        let response = envelope["response"].as_str().unwrap_or_default().to_owned();
        if response.trim().is_empty() {
            return Err(AppError::HarnessFailed {
                reason: "Antigravity completed without a final result".into(),
                stderr_tail: diagnostics_tail(&stderr, &unexpected_stdout),
            });
        }
        if !envelope["usage"].is_null() {
            let usage = &envelope["usage"];
            model_calls.push(ModelCallUsage {
                call_id: "antigravity:turn-0".into(),
                provider: Some("google".into()),
                api: Some("antigravity-cli-turn".into()),
                model: requested_model.clone(),
                requested_model,
                input_tokens: usage["input_tokens"].as_u64(),
                output_tokens: usage["output_tokens"].as_u64(),
                reasoning_tokens: usage["thinking_tokens"].as_u64(),
                cache_read_tokens: usage["cache_read_tokens"].as_u64(),
                total_tokens: usage["total_tokens"].as_u64(),
                started_at: Some(started_at),
                ended_at: Some(chrono::Utc::now()),
                duration_millis: Some(started.elapsed().as_millis() as u64),
                stop_reason: Some(status.to_ascii_lowercase()),
                ..Default::default()
            });
        }
        telemetry.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        let _ = request
            .progress_tx
            .send(progress(&telemetry, &model_calls, &posts, &response));
        Ok(HarnessOutcome {
            final_text: response,
            envelope: None,
            stderr_tail: stderr_tail(&stderr),
        })
    }
}

fn progress(
    telemetry: &crate::harness::ActivityTelemetry,
    calls: &[ModelCallUsage],
    posts: &[String],
    response: &str,
) -> LiveProgress {
    LiveProgress {
        telemetry: telemetry.clone(),
        model_calls: calls.to_vec(),
        posts: posts
            .iter()
            .enumerate()
            .map(|(i, text)| LivePost {
                id: (0, i),
                kind: "tool".into(),
                text: text.clone(),
            })
            .collect(),
        response: response.into(),
        ..Default::default()
    }
}
