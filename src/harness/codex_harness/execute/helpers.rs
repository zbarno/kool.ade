use std::path::Path;

use serde_json::Value;

use crate::harness::{ExecutionMode, PlanningRequest};

pub(super) fn stderr_tail(lines: &[String]) -> String {
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
    isolated_review_workspace: &Path,
) -> (Vec<String>, String) {
    let mut argv = vec![
        binary.to_string_lossy().into_owned(),
        "exec".into(),
        "--json".into(),
    ];
    let working_directory = if request.mode == ExecutionMode::RuntimeQaVisualReview {
        isolated_review_workspace
    } else {
        &request.repo_root
    };
    argv.extend([
        "--cd".into(),
        working_directory.to_string_lossy().into_owned(),
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
