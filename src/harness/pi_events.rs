//! Folding pi's NDJSON event stream (docs/json.md) into the pieces the
//! planner needs: the authoritative final assistant text, a live activity
//! preview, completion/error signals.
//!
//! Deliberately tolerant: unparsable lines are counted and forgotten — pi
//! may print startup chatter that is not JSON.

use super::LiveProgress;
use serde_json::Value;
use std::collections::BTreeMap;

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
    blocks: BTreeMap<usize, (String, String)>,
    history: Vec<super::LivePost>,
    message_id: u64,
    prior_thoughts: String,
    prior_response: String,
}

impl EventFold {
    pub fn preview(&self) -> LiveProgress {
        let current_text = self.block_text("text");
        let (response, specification) = super::live_preview::project(&current_text);
        LiveProgress {
            posts: self
                .history
                .iter()
                .cloned()
                .chain(self.current_posts())
                .collect(),
            thoughts: joined(&self.prior_thoughts, &self.block_text("thinking")),
            response: joined(&self.prior_response, &response),
            specification,
            activity: self.last_activity.clone(),
        }
    }

    fn current_posts(&self) -> Vec<super::LivePost> {
        self.blocks
            .iter()
            .filter_map(|(index, (kind, text))| {
                let text = if kind == "text" {
                    super::live_preview::project(text).0
                } else {
                    text.clone()
                };
                if text.is_empty() {
                    return None;
                }
                Some(super::LivePost {
                    id: (self.message_id, *index),
                    kind: kind.clone(),
                    text,
                })
            })
            .collect()
    }

    fn block_text(&self, kind: &str) -> String {
        self.blocks
            .values()
            .filter(|(k, _)| k == kind)
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn joined(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_owned(),
        (_, true) => a.to_owned(),
        _ => format!("{a}\n\n{b}"),
    }
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
    None
}

/// Consume one stdout line into the fold.
pub fn fold_line(line: &str, sink: &mut EventFold) {
    static NEXT_MESSAGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    if sink.message_id == 0 {
        sink.message_id = NEXT_MESSAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if skip_heavy(line) == Some(true) && !sink.final_assistant_text.is_empty() {
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
        "agent_end" => {
            sink.saw_agent_end = true;
            if sink.final_assistant_text.is_empty() {
                if let Some(message) = v
                    .get("messages")
                    .and_then(Value::as_array)
                    .and_then(|messages| messages.last())
                {
                    if message.get("role").and_then(Value::as_str) == Some("assistant")
                        && !matches!(
                            message.get("stopReason").and_then(Value::as_str),
                            Some("toolUse" | "error" | "aborted")
                        )
                    {
                        sink.final_assistant_text = assistant_text(Some(message));
                    }
                }
            }
        }
        "tool_execution_start" => {
            let tool = v.get("toolName").and_then(Value::as_str).unwrap_or("tool");
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
        "message_start" => {
            if v.pointer("/message/role").and_then(Value::as_str) == Some("assistant") {
                sink.final_assistant_text.clear();
                let preview = sink.preview();
                sink.prior_thoughts = preview.thoughts;
                sink.prior_response = preview.response;
                sink.history.extend(sink.current_posts());
                sink.blocks.clear();
                sink.message_id = NEXT_MESSAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        "message_update" => {
            if let Some(event) = v.get("assistantMessageEvent") {
                let ty = event.get("type").and_then(Value::as_str).unwrap_or("");
                let kind = if ty.starts_with("thinking_") {
                    "thinking"
                } else if ty.starts_with("text_") {
                    "text"
                } else {
                    return;
                };
                let index = event
                    .get("contentIndex")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                let block = sink
                    .blocks
                    .entry(index)
                    .or_insert_with(|| (kind.to_owned(), String::new()));
                if ty.ends_with("_delta") {
                    if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                        block.1.push_str(delta);
                    }
                } else if ty.ends_with("_end") {
                    if let Some(content) = event.get("content").and_then(Value::as_str) {
                        block.1 = content.to_owned();
                    }
                }
            }
        }
        "message_end" => {
            let msg = v.get("message");
            if msg.and_then(|m| m.get("role")).and_then(Value::as_str) == Some("assistant") {
                sink.final_assistant_text = if matches!(
                    msg.and_then(|m| m.get("stopReason"))
                        .and_then(Value::as_str),
                    Some("toolUse" | "error" | "aborted")
                ) {
                    String::new()
                } else {
                    assistant_text(msg)
                };
                if let Some(blocks) = msg.and_then(|m| m.get("content")).and_then(Value::as_array) {
                    for (index, b) in blocks.iter().enumerate() {
                        let kind = b.get("type").and_then(Value::as_str).unwrap_or("");
                        if kind == "text" || kind == "thinking" {
                            if let Some(text) = b.get(kind).and_then(Value::as_str) {
                                sink.blocks
                                    .insert(index, (kind.to_owned(), text.to_owned()));
                            }
                        }
                    }
                } else {
                    sink.blocks
                        .insert(0, ("text".into(), sink.final_assistant_text.clone()));
                }
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
        let big = format!(
            r#"{{"type":"agent_end","messages":["{}"]}}"#,
            "x".repeat(500_000)
        );
        fold_line(&big, &mut f);
        assert!(f.saw_agent_end);
        fold_line("npm WARN something", &mut f);
        assert_eq!(f.unparsed_lines, 0);
        assert_eq!(f.events_seen, 1);
    }
    fn delta(f: &mut EventFold, kind: &str, index: usize, value: &str) {
        fold_line(
            &serde_json::json!({"type":"message_update", "assistantMessageEvent":{
                "type":format!("{kind}_delta"), "contentIndex":index, "delta":value
            }})
            .to_string(),
            f,
        );
    }

    #[test]
    fn deltas_stream_thoughts_and_spec_before_message_end() {
        let mut f = EventFold::default();
        delta(&mut f, "thinking", 0, "Checking ");
        delta(&mut f, "thinking", 0, "requirements.");
        delta(
            &mut f,
            "text",
            1,
            "Drafting.\n```json\n{\"assistantMessage\":\"New draft\",\"updatedSpecification\":\"# Scope\\n",
        );
        let preview = f.preview();
        assert_eq!(preview.thoughts, "Checking requirements.");
        assert_eq!(preview.response, "New draft");
        assert_eq!(preview.specification.as_deref(), Some("# Scope\n"));
        assert!(f.final_assistant_text.is_empty());
        assert!(!f.saw_agent_end);
        delta(&mut f, "text", 1, "More detail\"}");
        assert_eq!(
            f.preview().specification.as_deref(),
            Some("# Scope\nMore detail")
        );
    }

    #[test]
    fn thought_posts_stay_after_preceding_messages_across_calls() {
        let mut fold = EventFold::default();
        delta(&mut fold, "thinking", 0, "First thought.");
        delta(&mut fold, "text", 1, "Reading files.");
        let original = fold.preview().posts;
        fold_line(
            r#"{"type":"message_start","message":{"role":"assistant"}}"#,
            &mut fold,
        );
        delta(&mut fold, "thinking", 0, "Next thought.");
        let mut display = fold.preview();
        assert_eq!(&display.posts[..2], &original);
        assert_eq!(
            display
                .posts
                .iter()
                .map(|p| p.kind.as_str())
                .collect::<Vec<_>>(),
            ["thinking", "text", "thinking"]
        );
        let next_id = display.posts[2].id;
        delta(&mut fold, "thinking", 0, " More detail.");
        display.update(fold.preview());
        assert_eq!(display.posts.len(), 3);
        assert_eq!(display.posts[2].id, next_id);
        assert_eq!(&display.posts[..2], &original);
        let mut second_call = EventFold::default();
        delta(&mut second_call, "thinking", 0, "New task.");
        display.update(second_call.preview());
        display.update(second_call.preview());
        assert_eq!(
            display.posts.len(),
            4,
            "snapshots update existing blocks without duplicating or replacing earlier calls"
        );
        assert_ne!(display.posts[3].id, original[0].id);
    }

    #[test]
    fn block_end_and_message_end_reconcile_without_duplicate_thoughts() {
        let mut f = EventFold::default();
        delta(&mut f, "thinking", 0, "First thought.");
        fold_line(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_end","contentIndex":0,"content":"First thought."}}"#,
            &mut f,
        );
        fold_line(
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"First thought."},{"type":"text","text":"Reading files."}]}}"#,
            &mut f,
        );
        assert_eq!(f.preview().thoughts, "First thought.");
        fold_line(
            r#"{"type":"message_start","message":{"role":"toolResult"}}"#,
            &mut f,
        );
        assert_eq!(f.preview().response, "Reading files.");
        fold_line(
            r#"{"type":"message_start","message":{"role":"assistant"}}"#,
            &mut f,
        );
        delta(&mut f, "thinking", 0, "Next thought.");
        assert_eq!(f.preview().thoughts, "First thought.\n\nNext thought.");
        assert_eq!(f.preview().response, "Reading files.");
    }

    #[test]
    fn large_final_documents_are_not_mistaken_for_agent_end() {
        let mut f = EventFold::default();
        let text = "x".repeat(300_000);
        fold_line(&text_msg(&text), &mut f);
        assert_eq!(f.final_assistant_text, text);
        assert!(!f.saw_agent_end);
    }
    #[test]
    fn agent_end_recovers_missing_message_end_but_not_unfinished_tools() {
        let mut fold = EventFold::default();
        fold_line(
            r#"{"type":"agent_end","messages":[{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"complete report"}]}]}"#,
            &mut fold,
        );
        assert_eq!(fold.final_assistant_text, "complete report");
        for last in [
            serde_json::json!({"role":"toolResult","content":"tool output"}),
            serde_json::json!({"role":"assistant","stopReason":"toolUse","content":[{"type":"text","text":"not final"}]}),
        ] {
            let mut fold = EventFold::default();
            fold_line(&serde_json::json!({"type":"agent_end","messages":[{"role":"assistant","content":"old summary"},last]}).to_string(), &mut fold);
            assert!(fold.final_assistant_text.is_empty());
        }
    }

    #[test]
    fn pending_assistant_does_not_reuse_an_earlier_completed_message() {
        let mut fold = EventFold::default();
        fold_line(&text_msg("old summary"), &mut fold);
        fold_line(
            r#"{"type":"message_start","message":{"role":"assistant"}}"#,
            &mut fold,
        );
        assert!(fold.final_assistant_text.is_empty());
    }
}
