use std::path::PathBuf;

#[derive(Default)]
pub(crate) struct CopilotUsage {
    pub(crate) model: Option<String>,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) total_tokens: Option<u64>,
}

pub(crate) struct UsageFile {
    pub(crate) path: PathBuf,
}

impl UsageFile {
    pub(crate) fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "koolade-copilot-usage-{}.json",
                uuid::Uuid::new_v4()
            )),
        }
    }

    pub(crate) fn read(&self) -> CopilotUsage {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .map(|value| parse_usage(&value))
            .unwrap_or_default()
    }
}

impl Drop for UsageFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn parse_usage(value: &serde_json::Value) -> CopilotUsage {
    CopilotUsage {
        model: find_value(value, &["model", "modelName"])
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        input_tokens: find_value(
            value,
            &[
                "inputTokens",
                "input_tokens",
                "promptTokens",
                "prompt_tokens",
            ],
        )
        .and_then(serde_json::Value::as_u64),
        output_tokens: find_value(
            value,
            &[
                "outputTokens",
                "output_tokens",
                "completionTokens",
                "completion_tokens",
            ],
        )
        .and_then(serde_json::Value::as_u64),
        total_tokens: find_value(value, &["totalTokens", "total_tokens"])
            .and_then(serde_json::Value::as_u64),
    }
}

fn find_value<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a serde_json::Value> {
    match value {
        serde_json::Value::Object(object) => object.iter().find_map(|(key, value)| {
            keys.iter()
                .any(|candidate| key.eq_ignore_ascii_case(candidate))
                .then_some(value)
                .or_else(|| find_value(value, keys))
        }),
        serde_json::Value::Array(items) => items.iter().find_map(|value| find_value(value, keys)),
        _ => None,
    }
}
