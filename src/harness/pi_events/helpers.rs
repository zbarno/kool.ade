use serde_json::Value;

pub(super) fn assistant_text(message: Option<&Value>) -> String {
    let Some(content) = message.and_then(|m| m.get("content")) else {
        return String::new();
    };
    let mut out = String::new();
    if let Some(blocks) = content.as_array() {
        for b in blocks {
            if b.get("type").and_then(Value::as_str) == Some("text")
                && let Some(t) = b.get("text").and_then(Value::as_str)
            {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(t);
            }
        }
    } else if let Some(t) = content.as_str() {
        out.push_str(t);
    }
    out
}

/// Compact, capped string form for previews/errors.
pub(super) fn short_blob(v: Option<&Value>) -> String {
    let s = v
        .and_then(|v| serde_json::to_string(v).ok())
        .unwrap_or_default();
    let s = s.replace('\\', "");
    const CAP: usize = 90;
    if s.chars().count() <= CAP {
        return s;
    }
    let mut out: String = s.chars().take(CAP.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Fast-path sniff for non-JSON chatter and the large final event payload.
pub(super) fn skip_heavy(line: &str) -> Option<bool> {
    let head: String = line.chars().take(64).collect();
    if !head.starts_with('{') {
        return Some(false);
    }
    if head.contains("\"agent_end\"") {
        return Some(true);
    }
    None
}

pub(super) fn classify_model_error(message: &str) -> &'static str {
    let message = message.to_ascii_lowercase();
    if [
        "context length",
        "context window",
        "maximum context",
        "too many tokens",
    ]
    .iter()
    .any(|part| message.contains(part))
    {
        "context_limit"
    } else if ["rate limit", "rate_limit", "too many requests", "http 429"]
        .iter()
        .any(|part| message.contains(part))
    {
        "rate_limited"
    } else if ["unauthorized", "authentication", "api key", "http 401"]
        .iter()
        .any(|part| message.contains(part))
    {
        "provider_authentication"
    } else if ["timeout", "timed out", "deadline", "aborted"]
        .iter()
        .any(|part| message.contains(part))
    {
        "provider_timeout"
    } else {
        "model_response_error"
    }
}
