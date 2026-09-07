//! Folding pi's NDJSON event stream (docs/json.md) into the pieces the
//! planner needs: the authoritative final assistant text, a live activity
//! preview, completion/error signals.
//!
//! Deliberately tolerant: unparsable lines are counted and forgotten — pi
//! may print startup chatter that is not JSON.

use serde_json::Value;

/// Rolling state accumulated line by line.
#[derive(Debug, Default, Clone)]
pub struct EventFold {
    /// Final assistant text from the LAST `message_end` (authoritative).
    pub final_assistant_text: String,
    /// Most recent human-readable activity (e.g. tool being executed).
    pub last_activity: Option<String>,
    pub saw_agent_end: bool,
    pub error_hint: Option<String>,
    pub events_seen: usize,
    pub unparsed_lines: usize,
}

/// Fast-path sniff: lines we do NOT need to fully parse (huge agent_end
/// payload, obviously not JSON).
fn skip_heavy(line: &str) -> Option<bool> {
    let head: String = line.chars().take(64).collect();
    if !head.starts_with('{') {
        return Some(false);
    }
    // agent_end embeds the FULL conversation; we only care that it happened.
    if head.contains("\"agent_end\"") {
        return Some(true);
    }
    if line.len() > 256 * 1024 {
        return Some(true);
    }
    None
}

/// Consume one stdout line into the fold.
pub fn fold_line(line: &str, sink: &mut EventFold) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if skip_heavy(line) == Some(true) {
        sink.saw_agent_end = true;
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        if line.starts_with('{') {
            sink.unparsed_lines += 1;
        }
        return;
    };
    sink.events_seen += 1;
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "agent_end" => sink.saw_agent_end = true,
        "tool_execution_start" => {
            let tool = v
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = v
                .get("args")
                .map(|a| short_blob(Some(a)))
                .unwrap_or_default();
            sink.last_activity = Some(if args.is_empty() {
                tool.to_string()
            } else {
                format!("{tool} · {args}")
            });
        }
        "message_end" => {
            let msg = v.get("message");
            if msg.and_then(|m| m.get("role")).and_then(Value::as_str) == Some("assistant") {
                sink.final_assistant_text = assistant_text(msg);
            }
        }
        t if t.contains("error") => {
            sink.error_hint = Some(short_blob(Some(&v)));
        }
        _ => {}
    }
}

/// Concatenate text content blocks of an assistant message value.
fn assistant_text(message: Option<&Value>) -> String {
    let Some(content) = message.and_then(|m| m.get("content")) else {
        return String::new();
    };
    let mut out = String::new();
    if let Some(blocks) = content.as_array() {
        for b in blocks {
            if b.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(t) = b.get("text").and_then(Value::as_str) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
        }
    } else if let Some(t) = content.as_str() {
        out.push_str(t);
    }
    out
}

/// Compact, capped string form for previews/errors.
pub fn short_blob(v: Option<&Value>) -> String {
    let s = v.and_then(|v| serde_json::to_string(v).ok()).unwrap_or_default();
    let s = s.replace('\\', "");
    const CAP: usize = 90;
    if s.chars().count() <= CAP {
        return s;
    }
    let mut out: String = s.chars().take(CAP.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_msg(text: &str) -> String {
        format!(
            "{{\"type\":\"message_end\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":{}}}]}}}}",
            serde_json::to_string(text).unwrap()
        )
    }

    #[test]
    fn tracks_final_assistant_text_last_wins() {
        let mut f = EventFold::default();
        fold_line(&text_msg("first draft"), &mut f);
        assert_eq!(f.final_assistant_text, "first draft");
        fold_line(&text_msg("FINAL words"), &mut f);
        assert_eq!(f.final_assistant_text, "FINAL words");
    }

    #[test]
    fn tool_activity_and_errors_recorded() {
        let mut f = EventFold::default();
        fold_line(
            r#"{"type":"tool_execution_start","toolCallId":"1","toolName":"bash","args":{"command":"ls src"}}"#,
            &mut f,
        );
        assert!(f.last_activity.as_deref().unwrap_or("").starts_with("bash"));
        fold_line(r#"{"type":"agent_error","error":"boom"}"#, &mut f);
        assert!(f.error_hint.as_deref().unwrap_or("").contains("boom"));
    }

    #[test]
    fn heavy_agent_end_and_chatter_are_safe() {
        let mut f = EventFold::default();
        let big = format!(r#"{{"type":"agent_end","messages":["{}"]}}"#, "x".repeat(500_000));
        fold_line(&big, &mut f);
        assert!(f.saw_agent_end);
        fold_line("npm WARN something", &mut f);
        assert_eq!(f.unparsed_lines, 0);
        assert_eq!(f.events_seen, 0);
    }
}
