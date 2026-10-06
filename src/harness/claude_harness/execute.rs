use std::time::{Duration, Instant};

use super::stream::{ClaudeEvent, command, parse_event, prompt_input};
use super::{CLAUDE_MODEL_ENV, ClaudeHarness};
use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
use crate::harness::{AiHarness, HarnessOutcome, LivePost, LiveProgress, PlanningRequest};

impl AiHarness for ClaudeHarness {
    fn label(&self) -> String {
        match self.check_available() {
            Ok(version) => format!("claude {version}"),
            Err(AppError::HarnessNotFound { .. }) => "claude (not installed)".into(),
            Err(error) => format!("claude (unavailable: {})", error.headline()),
        }
    }

    fn check_available(&self) -> Result<String, AppError> {
        Self::checked_binary(&Self::locate_binary()?)
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let binary = Self::locate_binary()?.canonicalize().map_err(|error| {
            AppError::Other(format!("Cannot resolve Claude Code executable: {error}"))
        })?;
        let argv = command(&binary, request);
        let mut developer = String::from(
            "Kool.ad/e owns task lifecycle, source and destination branches, approvals, Git operations, and publication. Never switch branches, commit, push, open a pull request, or publish. Work only on the task requested below and return its required structured result.",
        );
        developer.push_str("\n\n");
        developer.push_str(&request.system_instructions);
        let mut argv = argv;
        argv.extend(["--append-system-prompt".into(), developer]);
        if let Some(model) = std::env::var(CLAUDE_MODEL_ENV)
            .ok()
            .filter(|model| !model.trim().is_empty())
        {
            argv.extend(["--model".into(), model]);
        }
        let effort = normalize_effort(&request.reasoning_level);
        argv.extend(["--effort".into(), effort.into()]);

        let task = crate::harness::pi_proc::spawn_with_input(
            &argv,
            &request.repo_root,
            Some(prompt_input(&request.prompt_body)),
        )?;
        let deadline = Instant::now() + request.timeout;
        let mut stderr = Vec::new();
        let mut exit_status = None;
        let mut final_text = String::new();
        let mut posts = Vec::new();
        let mut telemetry = crate::harness::ActivityTelemetry {
            started_ms: Some(chrono::Utc::now().timestamp_millis()),
            model: std::env::var(CLAUDE_MODEL_ENV)
                .ok()
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
                            ClaudeEvent::Text(text) => final_text = text,
                            ClaudeEvent::Tool(name) => posts.push(name),
                            ClaudeEvent::Completed {
                                text,
                                input,
                                output,
                                cost,
                            } => {
                                final_text = text;
                                telemetry.input_tokens = input.or(telemetry.input_tokens);
                                telemetry.output_tokens = output.or(telemetry.output_tokens);
                                telemetry.cost_microusd = cost.or(telemetry.cost_microusd);
                            }
                            ClaudeEvent::Usage {
                                input,
                                output,
                                cost,
                                model,
                            } => {
                                telemetry.input_tokens = input.or(telemetry.input_tokens);
                                telemetry.output_tokens = output.or(telemetry.output_tokens);
                                telemetry.cost_microusd = cost.or(telemetry.cost_microusd);
                                telemetry.model = model.or_else(|| telemetry.model.clone());
                            }
                            ClaudeEvent::Failure(reason) => {
                                return Err(AppError::HarnessFailed {
                                    reason,
                                    stderr_tail: stderr_tail(&stderr),
                                });
                            }
                            ClaudeEvent::Other => {}
                        }
                        let _ = request
                            .progress_tx
                            .send(snapshot(&telemetry, &posts, &final_text));
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
                    "Claude Code exited with a failure status".into()
                } else {
                    "Claude Code output stream ended before process completion".into()
                },
                stderr_tail: stderr_tail(&stderr),
            });
        }
        if final_text.trim().is_empty() {
            return Err(AppError::HarnessFailed {
                reason: "Claude Code completed without a final result".into(),
                stderr_tail: stderr_tail(&stderr),
            });
        }
        telemetry.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        let _ = request
            .progress_tx
            .send(snapshot(&telemetry, &posts, &final_text));
        Ok(HarnessOutcome {
            final_text,
            envelope: None,
            stderr_tail: stderr_tail(&stderr),
        })
    }
}

fn normalize_effort(value: &str) -> &'static str {
    match value.to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" | "xhigh" => "high",
        _ => "medium",
    }
}

fn snapshot(
    telemetry: &crate::harness::ActivityTelemetry,
    names: &[String],
    response: &str,
) -> LiveProgress {
    LiveProgress {
        telemetry: telemetry.clone(),
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
