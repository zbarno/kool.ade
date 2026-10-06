use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{CODEX_MODEL_ENV, CodexHarness, STDERR_LIMIT};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{
    AiHarness, ExecutionMode, HarnessOutcome, LivePost, LiveProgress, ModelCallUsage,
    PlanningRequest,
};

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
        let binary = Self::locate_binary()?.canonicalize().map_err(|error| {
            AppError::Other(format!("Cannot resolve Codex CLI executable: {error}"))
        })?;
        let requested_model = model.map(Into::into).or_else(|| {
            std::env::var(CODEX_MODEL_ENV)
                .ok()
                .filter(|model| !model.trim().is_empty())
                .map(Into::into)
        });
        let requested_model_name = requested_model
            .as_ref()
            .map(|model: &std::ffi::OsString| model.to_string_lossy().into_owned());
        let (argv, prompt) = command(&binary, request, requested_model);
        let task =
            crate::harness::pi_proc::spawn_with_input(&argv, &request.repo_root, Some(prompt))?;
        let deadline = Instant::now() + request.timeout;
        let mut stderr = Vec::new();
        let mut exit_status = None;
        let mut final_text = String::new();
        let mut activity = Vec::new();
        let mut model_calls = Vec::new();
        let started_at = chrono::Utc::now();
        let started = Instant::now();
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
                            CodexEvent::Message(text) => final_text = text,
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
                                return Err(AppError::HarnessFailed {
                                    reason,
                                    stderr_tail: stderr_tail(&stderr),
                                });
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

#[derive(Debug, PartialEq, Eq)]
pub enum CodexEvent {
    Message(String),
    Activity(String),
    Usage {
        call_id: Option<String>,
        input: Option<u64>,
        output: Option<u64>,
        cached_input: Option<u64>,
        cache_write_input: Option<u64>,
    },
    Failure(String),
    Other,
}

pub fn parse_event(line: &str) -> Option<CodexEvent> {
    let value: Value = serde_json::from_str(line).ok()?;
    match value["type"].as_str()? {
        "item.completed" => {
            let item = &value["item"];
            match item["type"].as_str()? {
                "agent_message" => item["text"]
                    .as_str()
                    .map(|text| CodexEvent::Message(text.to_owned())),
                "command_execution" => Some(CodexEvent::Activity(
                    item["command"].as_str().unwrap_or("Ran command").to_owned(),
                )),
                _ => Some(CodexEvent::Other),
            }
        }
        "item.started" => Some(CodexEvent::Activity(
            value["item"]["type"]
                .as_str()
                .unwrap_or("Working")
                .to_owned(),
        )),
        "turn.completed" => Some(CodexEvent::Usage {
            call_id: value["turn_id"]
                .as_str()
                .map(|turn| format!("codex:{turn}")),
            input: value["usage"]["input_tokens"].as_u64(),
            output: value["usage"]["output_tokens"].as_u64(),
            cached_input: value["usage"]["cached_input_tokens"].as_u64(),
            cache_write_input: value["usage"]["cache_write_input_tokens"].as_u64(),
        }),
        "turn.failed" | "error" => Some(CodexEvent::Failure(
            value["error"]["message"]
                .as_str()
                .unwrap_or("Codex CLI reported an error")
                .to_owned(),
        )),
        _ => Some(CodexEvent::Other),
    }
}

pub fn normalize_effort(value: &str) -> &'static str {
    match value.to_ascii_lowercase().as_str() {
        "off" => "minimal",
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        "xhigh" => "xhigh",
        _ => "medium",
    }
}

pub fn command(
    binary: &Path,
    request: &PlanningRequest,
    model: Option<std::ffi::OsString>,
) -> (Vec<String>, String) {
    let mut argv = vec![
        binary.to_string_lossy().into_owned(),
        "exec".into(),
        "--json".into(),
    ];
    argv.extend([
        "--cd".into(),
        request.repo_root.to_string_lossy().into_owned(),
    ]);
    let sandbox = if request.mode == ExecutionMode::Implementation {
        "workspace-write"
    } else {
        "read-only"
    };
    argv.extend(["--sandbox".into(), sandbox.into(), "--ephemeral".into()]);
    let model = model.filter(|value| !value.is_empty());
    if let Some(value) = &model {
        argv.extend(["--model".into(), value.to_string_lossy().into_owned()]);
    }
    let effort = normalize_effort(&request.reasoning_level);
    argv.extend([
        "--config".into(),
        format!("model_reasoning_effort=\"{effort}\""),
    ]);
    let developer = format!(
        "Kool.ad/e operation: {:?}. The application owns task lifecycle, branch selection, approvals, Git operations, and publication. Do not change branches or publish. Follow the requested operation and return the exact structured response requested by the application.\n\n{}",
        request.mode, request.system_instructions
    );
    let developer = serde_json::to_string(&developer).expect("string serialization is infallible");
    argv.extend([
        "--config".into(),
        format!("developer_instructions={developer}"),
    ]);
    let prompt = request.prompt_body.clone();
    (argv, prompt)
}
