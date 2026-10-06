use std::path::Path;

use serde_json::Value;

use crate::harness::{ExecutionMode, PlanningRequest, ToolAccess};

pub(crate) fn command(binary: &Path, request: &PlanningRequest) -> Vec<String> {
    let mut args = vec![
        binary.to_string_lossy().into_owned(),
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--no-session-persistence".into(),
        "--restricted".into(),
        "--permission-prompts".into(),
        "none".into(),
        "--tools".into(),
    ];
    let tools = if request.mode == ExecutionMode::Implementation {
        "Read,Glob,Grep,Edit,Write"
    } else if request.mode.tool_access() == ToolAccess::None {
        ""
    } else {
        "Read,Glob,Grep"
    };
    args.push(tools.into());
    args.extend([
        "--permission-mode".into(),
        if request.mode == ExecutionMode::Implementation {
            "acceptEdits".into()
        } else {
            "plan".into()
        },
    ]);
    args
}

pub(crate) fn prompt_input(prompt: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": prompt}]
        }
    })
    .to_string()
        + "\n"
}

#[derive(Debug, PartialEq)]
pub(crate) enum ClaudeEvent {
    Text(String),
    Tool(String),
    Completed {
        text: String,
        input: Option<u64>,
        output: Option<u64>,
        cost: Option<u64>,
    },
    Usage {
        input: Option<u64>,
        output: Option<u64>,
        cost: Option<u64>,
        model: Option<String>,
    },
    Failure(String),
    Other,
}

pub(crate) fn parse_event(line: &str) -> Option<ClaudeEvent> {
    let value: Value = serde_json::from_str(line).ok()?;
    match value["type"].as_str()? {
        "system" if value["subtype"] == "init" => Some(ClaudeEvent::Usage {
            input: None,
            output: None,
            cost: None,
            model: value["model"].as_str().map(str::to_owned),
        }),
        "assistant" => {
            let content = value["message"]["content"].as_array()?;
            let mut emitted = None;
            for block in content {
                match block["type"].as_str()? {
                    "text" => {
                        if let Some(text) = block["text"].as_str() {
                            emitted = Some(ClaudeEvent::Text(text.to_owned()));
                        }
                    }
                    "tool_use" => {
                        emitted = Some(ClaudeEvent::Tool(
                            block["name"].as_str().unwrap_or("tool").to_owned(),
                        ));
                    }
                    _ => {}
                }
            }
            emitted.or(Some(ClaudeEvent::Other))
        }
        "result" if value["is_error"] == true => Some(ClaudeEvent::Failure(
            value["result"]
                .as_str()
                .unwrap_or("Claude Code reported an error")
                .to_owned(),
        )),
        "result" => Some(ClaudeEvent::Completed {
            text: value["result"].as_str().unwrap_or_default().to_owned(),
            input: value["usage"]["input_tokens"].as_u64(),
            output: value["usage"]["output_tokens"].as_u64(),
            cost: value["total_cost_usd"]
                .as_f64()
                .map(|cost| (cost * 1_000_000.0).round() as u64),
        }),
        _ => Some(ClaudeEvent::Other),
    }
}
