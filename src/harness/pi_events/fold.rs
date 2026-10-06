use serde_json::Value;

use super::EventFold;
use super::helpers::{assistant_text, classify_model_error, short_blob, skip_heavy};
use std::time::Instant;

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
            if let Some(message) = v
                .get("messages")
                .and_then(Value::as_array)
                .and_then(|messages| messages.last())
                && message.get("role").and_then(Value::as_str) == Some("assistant")
            {
                sink.last_stop_reason = message
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .filter(|reason| matches!(*reason, "stop" | "toolUse" | "error" | "aborted"))
                    .map(str::to_owned);
                sink.last_error_class = message
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .map(classify_model_error);
                if sink.final_assistant_text.is_empty()
                    && !matches!(
                        message.get("stopReason").and_then(Value::as_str),
                        Some("toolUse" | "error" | "aborted")
                    )
                {
                    sink.final_assistant_text = assistant_text(Some(message));
                }
            }
        }
        "tool_execution_start" => {
            sink.tool_executions += 1;
            let tool = v.get("toolName").and_then(Value::as_str).unwrap_or("tool");
            let args = v
                .get("args")
                .map(|a| short_blob(Some(a)))
                .unwrap_or_default();
            let preview = sink.preview();
            sink.prior_thoughts = preview.thoughts;
            sink.prior_response = preview.response;
            sink.history.extend(sink.current_posts());
            sink.blocks.clear();
            let id = (
                NEXT_MESSAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                0,
            );
            let call = v
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            sink.tool_posts.insert(call, id);
            sink.history.push(super::super::LivePost {
                id,
                kind: "tool".into(),
                text: format!(
                    "{tool}\n{}",
                    v.get("args").map(|v| v.to_string()).unwrap_or_default()
                ),
            });
            sink.last_activity = Some(if args.is_empty() {
                tool.to_string()
            } else {
                format!("{tool} · {args}")
            });
        }
        "tool_execution_update" | "tool_execution_end" => {
            let call = v
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool = v.get("toolName").and_then(Value::as_str).unwrap_or("tool");
            let output = assistant_text(v.get("result").or_else(|| v.get("partialResult")));
            if let Some(id) = sink.tool_posts.get(call)
                && let Some(post) = sink.history.iter_mut().find(|post| &post.id == id)
            {
                // Pi sends cumulative partialResult snapshots, followed by the final result.
                if !output.is_empty() {
                    let header = post.text.split("\n\nOutput:\n").next().unwrap_or(tool);
                    post.text = format!("{header}\n\nOutput:\n{output}");
                }
            }
            sink.last_activity = Some(format!(
                "{tool} · {}",
                if ty == "tool_execution_update" {
                    "running"
                } else if v.get("isError").and_then(Value::as_bool) == Some(true) {
                    "failed"
                } else {
                    "completed"
                }
            ));
        }
        "message_start" => {
            if v.pointer("/message/role").and_then(Value::as_str) == Some("assistant") {
                let call_id = super::usage::call_id(v.get("message"), sink.model_calls.len());
                sink.call_started = Some((call_id, chrono::Utc::now(), Instant::now()));
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
            if v.get("usage").is_some() {
                let call_id = sink
                    .call_started
                    .as_ref()
                    .map(|(id, _, _)| id.clone())
                    .unwrap_or_else(|| {
                        super::usage::call_id(v.get("message"), sink.model_calls.len())
                    });
                let mut call = super::usage::parse_update(&v, call_id.clone());
                if let Some((_, started_at, started)) = &sink.call_started {
                    call.started_at = Some(*started_at);
                    call.duration_millis =
                        Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
                }
                if let Some(existing) = sink
                    .model_calls
                    .iter_mut()
                    .find(|item| item.call_id == call_id)
                {
                    *existing = super::usage::merge(call, existing);
                } else {
                    sink.model_calls.push(call);
                }
            }
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
                } else if ty.ends_with("_end")
                    && let Some(content) = event.get("content").and_then(Value::as_str)
                {
                    block.1 = content.to_owned();
                }
                let text = sink.block_text("text");
                if let Some(checklist) = super::parse_checklist_marker(&text)
                    && checklist != sink.checklist
                {
                    sink.checklist = checklist;
                    sink.checklist_revision = sink.checklist_revision.saturating_add(1);
                }
            }
        }
        "message_end" => {
            let msg = v.get("message");
            if msg.and_then(|m| m.get("role")).and_then(Value::as_str) == Some("assistant") {
                let (call_id, started_at, started) =
                    sink.call_started.take().unwrap_or_else(|| {
                        (
                            super::usage::call_id(msg, sink.model_calls.len()),
                            chrono::Utc::now(),
                            Instant::now(),
                        )
                    });
                let mut call = super::usage::parse(msg, call_id.clone());
                call.started_at = Some(started_at);
                call.ended_at = Some(chrono::Utc::now());
                call.duration_millis =
                    Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
                call.stop_reason = msg
                    .and_then(|message| message.get("stopReason"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(existing) = sink
                    .model_calls
                    .iter_mut()
                    .find(|item| item.call_id == call_id)
                {
                    *existing = super::usage::merge(call, existing);
                } else {
                    sink.model_calls.push(call);
                }
                sink.last_stop_reason = msg
                    .and_then(|message| message.get("stopReason"))
                    .and_then(Value::as_str)
                    .filter(|reason| matches!(*reason, "stop" | "toolUse" | "error" | "aborted"))
                    .map(str::to_owned);
                sink.last_error_class = msg
                    .and_then(|message| message.get("errorMessage"))
                    .and_then(Value::as_str)
                    .map(classify_model_error);
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
                        if (kind == "text" || kind == "thinking")
                            && let Some(text) = b.get(kind).and_then(Value::as_str)
                        {
                            sink.blocks
                                .insert(index, (kind.to_owned(), text.to_owned()));
                        }
                    }
                } else {
                    sink.blocks
                        .insert(0, ("text".into(), sink.final_assistant_text.clone()));
                }
                let text = sink.block_text("text");
                if let Some(checklist) = super::parse_checklist_marker(&text)
                    && checklist != sink.checklist
                {
                    sink.checklist = checklist;
                    sink.checklist_revision = sink.checklist_revision.saturating_add(1);
                }
            }
        }
        t if t.contains("error") => {
            sink.error_hint = Some(short_blob(Some(&v)));
        }
        _ => {}
    }
}
