use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{CODEX_MODEL_ENV, CodexHarness, STDERR_LIMIT};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{
    AiHarness, ExecutionMode, HarnessOutcome, LivePost, LiveProgress, PlanningRequest,
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
        let binary = Self::locate_binary()?.canonicalize().map_err(|error| {
            AppError::Other(format!("Cannot resolve Codex CLI executable: {error}"))
        })?;
        let model = request
            .model
            .clone()
            .filter(|model| !model.trim().is_empty())
            .or_else(|| std::env::var(CODEX_MODEL_ENV).ok())
            .map(Into::into);
        let (argv, prompt) = command(&binary, request, model);
        let task =
            crate::harness::pi_proc::spawn_with_input(&argv, &request.repo_root, Some(prompt))?;
        let deadline = Instant::now() + request.timeout;
        let mut stderr = Vec::new();
        let mut exit_status = None;
        let mut final_text = String::new();
        let mut activity = Vec::new();
        let mut telemetry = crate::harness::ActivityTelemetry {
            started_ms: Some(chrono::Utc::now().timestamp_millis()),
            model: request
                .model
                .clone()
                .filter(|model| !model.trim().is_empty())
                .or_else(|| std::env::var(CODEX_MODEL_ENV).ok())
                .filter(|model| !model.trim().is_empty()),
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
                            CodexEvent::Usage { input, output } => {
                                telemetry.input_tokens = input.or(telemetry.input_tokens);
                                telemetry.output_tokens = output.or(telemetry.output_tokens);
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
        input: Option<u64>,
        output: Option<u64>,
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
            input: value["usage"]["input_tokens"].as_u64(),
            output: value["usage"]["output_tokens"].as_u64(),
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
