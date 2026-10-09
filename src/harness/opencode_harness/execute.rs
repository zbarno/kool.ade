use std::time::{Duration, Instant};

use super::OpenCodeHarness;
use super::stream::{OpenCodeEvent, parse_event, permission_policy};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{
    AiHarness, HarnessOutcome, LivePost, LiveProgress, ModelCallUsage, PlanningRequest,
};

impl AiHarness for OpenCodeHarness {
    fn label(&self) -> String {
        match self.check_available() {
            Ok(version) => format!("opencode {version}"),
            Err(AppError::HarnessNotFound { .. }) => "opencode (not installed)".into(),
            Err(error) => format!("opencode (unavailable: {})", error.headline()),
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
        crate::harness::require_application_implementation_boundary("OpenCode", request)?;
        let binary = Self::locate_binary()?.canonicalize().map_err(|error| {
            AppError::Other(format!("Cannot resolve OpenCode executable: {error}"))
        })?;
        let mut argv = vec![
            binary.to_string_lossy().into_owned(),
            "--pure".into(),
            "run".into(),
            "--format".into(),
            "json".into(),
            "--auto".into(),
            "--dir".into(),
            request.repo_root.to_string_lossy().into_owned(),
        ];
        let model = model
            .filter(|model| !model.trim().is_empty())
            .map(str::to_owned);
        if let Some(model) = &model {
            argv.extend(["--model".into(), model.clone()]);
        }
        let mut prompt = String::from(
            "Kool.ad/e owns task lifecycle, source and destination branches, approvals, Git operations, and publication. Do not switch branches, commit, push, open a pull request, or publish. Work only on the task requested below and return its required structured result.\n\n",
        );
        prompt.push_str("Task policy:\n");
        prompt.push_str(&request.system_instructions);
        prompt.push_str("\n\nTask:\n");
        prompt.push_str(&request.prompt_body);
        let env = vec![
            (
                "OPENCODE_PERMISSION".into(),
                permission_policy(request.mode.tool_access()),
            ),
            ("OPENCODE_DISABLE_DEFAULT_PLUGINS".into(), "1".into()),
            ("OPENCODE_DISABLE_AUTOUPDATE".into(), "1".into()),
            ("OPENCODE_DISABLE_CLAUDE_CODE".into(), "1".into()),
        ];
        let task = crate::harness::pi_proc::spawn_with_input_env(
            &argv,
            &request.repo_root,
            Some(prompt),
            &env,
        )?;
        let deadline = Instant::now() + request.timeout;
        let mut stderr = Vec::new();
        let mut exit_status = None;
        let mut final_text = String::new();
        let mut posts = Vec::new();
        let mut model_calls = Vec::new();
        let started_at = chrono::Utc::now();
        let mut step_started_at = started_at;
        let mut step_started = Instant::now();
        let mut telemetry = crate::harness::ActivityTelemetry {
            started_ms: Some(started_at.timestamp_millis()),
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
                            OpenCodeEvent::Text(text) => final_text.push_str(&text),
                            OpenCodeEvent::Tool(name) => posts.push(name),
                            OpenCodeEvent::StepFinished {
                                call_id,
                                provider,
                                model: actual_model,
                                input,
                                output,
                                cost_microusd,
                                stop_reason,
                            } => {
                                let end = chrono::Utc::now();
                                // OpenCode emits one step_finish per completed step.
                                // Its duration is incremental, not time since this
                                // invocation began (which would double-count later
                                // steps in telemetry reports).
                                let duration_millis =
                                    step_started.elapsed().as_millis().min(u128::from(u64::MAX))
                                        as u64;
                                let call_started_at = step_started_at;
                                step_started_at = end;
                                step_started = Instant::now();
                                let requested_model = model.clone();
                                let provider = provider.or_else(|| {
                                    requested_model
                                        .as_deref()
                                        .and_then(|name| name.split_once('/').map(|(id, _)| id))
                                        .map(str::to_owned)
                                });
                                let model = actual_model.or_else(|| {
                                    requested_model.as_deref().and_then(|name| {
                                        name.split_once('/').map(|(_, id)| id.to_owned())
                                    })
                                });
                                let total_tokens = input.and_then(|input| {
                                    output.and_then(|output| input.checked_add(output))
                                });
                                model_calls.push(ModelCallUsage {
                                    call_id: call_id.unwrap_or_else(|| {
                                        format!("opencode:call-{}", model_calls.len())
                                    }),
                                    provider,
                                    model,
                                    requested_model,
                                    input_tokens: input,
                                    output_tokens: output,
                                    total_tokens,
                                    estimated_cost_usd_micros: cost_microusd,
                                    started_at: Some(call_started_at),
                                    ended_at: Some(end),
                                    duration_millis: Some(duration_millis),
                                    stop_reason,
                                    ..Default::default()
                                });
                            }
                            OpenCodeEvent::Failure(reason) => {
                                return Err(AppError::HarnessFailed {
                                    reason,
                                    stderr_tail: stderr_tail(&stderr),
                                });
                            }
                            OpenCodeEvent::Other => {}
                        }
                        let _ = request.progress_tx.send(snapshot(
                            &telemetry,
                            &model_calls,
                            &posts,
                            &final_text,
                        ));
                    }
                }
                Ok(StreamEvt::Stderr(line)) => {
                    stderr.push(line);
                    if stderr.len() > 100 {
                        stderr.remove(0);
                    }
                }
                Ok(StreamEvt::Exited(success)) => exit_status = Some(success),
                Err(PollState::Closed) => break,
                Err(PollState::Pending) => {}
            }
        }
        if exit_status != Some(true) {
            return Err(AppError::HarnessFailed {
                reason: if exit_status == Some(false) {
                    "OpenCode exited with a failure status".into()
                } else {
                    "OpenCode output stream ended before process completion".into()
                },
                stderr_tail: stderr_tail(&stderr),
            });
        }
        // OpenCode versions can exit successfully without a final step_finish
        // event, so process success plus assistant text is authoritative.
        if final_text.trim().is_empty() {
            return Err(AppError::HarnessFailed {
                reason: "OpenCode exited without a completed final response".into(),
                stderr_tail: stderr_tail(&stderr),
            });
        }
        telemetry.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        let _ = request
            .progress_tx
            .send(snapshot(&telemetry, &model_calls, &posts, &final_text));
        Ok(HarnessOutcome {
            final_text,
            envelope: None,
            stderr_tail: stderr_tail(&stderr),
        })
    }
}

fn snapshot(
    telemetry: &crate::harness::ActivityTelemetry,
    model_calls: &[ModelCallUsage],
    names: &[String],
    response: &str,
) -> LiveProgress {
    LiveProgress {
        telemetry: telemetry.clone(),
        model_calls: model_calls.to_vec(),
        posts: names
            .iter()
            .enumerate()
            .map(|(index, name)| LivePost {
                id: (0, index),
                kind: "tool".into(),
                text: name.clone(),
            })
            .collect(),
        activity: names.last().cloned(),
        response: response.to_owned(),
        ..LiveProgress::default()
    }
}

fn stderr_tail(lines: &[String]) -> String {
    let joined = lines.join("\n");
    const LIMIT: usize = 16_000;
    if joined.chars().count() <= LIMIT {
        return joined;
    }
    let mut tail = joined.chars().rev().take(LIMIT).collect::<Vec<_>>();
    tail.reverse();
    tail.into_iter().collect()
}
