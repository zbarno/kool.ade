use serde_json::Value;

#[derive(Debug, PartialEq)]
pub(super) enum OpenCodeEvent {
    Text(String),
    Tool(String),
    StepFinished {
        call_id: Option<String>,
        provider: Option<String>,
        model: Option<String>,
        input: Option<u64>,
        output: Option<u64>,
        cost_microusd: Option<u64>,
        stop_reason: Option<String>,
    },
    Failure(String),
    Other,
}

pub(super) fn parse_event(line: &str) -> Option<OpenCodeEvent> {
    let value: Value = serde_json::from_str(line).ok()?;
    match value["type"].as_str()? {
        "text" => value["part"]["text"]
            .as_str()
            .map(|text| OpenCodeEvent::Text(text.to_owned())),
        "tool_use" => Some(OpenCodeEvent::Tool(
            value["part"]["tool"].as_str().unwrap_or("tool").to_owned(),
        )),
        "step_finish" => {
            let tokens = &value["part"]["tokens"];
            let part = &value["part"];
            Some(OpenCodeEvent::StepFinished {
                call_id: value["sessionID"]
                    .as_str()
                    .zip(part["messageID"].as_str())
                    .map(|(session, message)| format!("opencode:{session}:{message}")),
                provider: part["providerID"].as_str().map(str::to_owned),
                model: part["modelID"].as_str().map(str::to_owned),
                input: tokens["input"].as_u64(),
                output: tokens["output"].as_u64(),
                cost_microusd: part["cost"]
                    .as_f64()
                    .filter(|cost| cost.is_finite() && *cost >= 0.0)
                    .map(|cost| (cost * 1_000_000.0).round().min(u64::MAX as f64) as u64),
                stop_reason: part["reason"].as_str().map(str::to_owned),
            })
        }
        "error" => Some(OpenCodeEvent::Failure(error_text(&value["error"]))),
        _ => Some(OpenCodeEvent::Other),
    }
}

fn error_text(value: &Value) -> String {
    value["data"]["message"]
        .as_str()
        .or_else(|| value["message"].as_str())
        .or_else(|| value.as_str())
        .unwrap_or("OpenCode reported an execution error")
        .to_owned()
}
