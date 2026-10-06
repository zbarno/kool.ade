use super::super::*;

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
fn provider_usage_is_normalized_and_repeated_snapshots_do_not_duplicate_calls() {
    let mut f = EventFold::default();
    let line = r#"{"type":"message_end","message":{"role":"assistant","id":"resp-1","provider":"anthropic","api":"messages","model":"sonnet","stopReason":"toolUse","usage":{"input":12,"output":4,"cacheRead":8,"cacheWrite":2,"reasoning":3,"totalTokens":26,"cost":{"total":0.001234}},"content":[]}}"#;
    fold_line(line, &mut f);
    fold_line(line, &mut f);
    let progress = f.preview();
    assert_eq!(progress.model_calls.len(), 1);
    assert_eq!(progress.model_calls[0].call_id, "resp-1");
    assert_eq!(progress.model_calls[0].input_tokens, Some(12));
    assert_eq!(
        progress.model_calls[0].estimated_cost_usd_micros,
        Some(1234)
    );
    assert_eq!(
        progress.model_calls[0].stop_reason.as_deref(),
        Some("toolUse")
    );
}

#[test]
fn cumulative_usage_updates_replace_tokens_and_survive_the_final_message() {
    let mut f = EventFold::default();
    fold_line(
        r#"{"type":"message_start","message":{"role":"assistant"}}"#,
        &mut f,
    );
    fold_line(
        r#"{"type":"message_update","usage":{"input":10,"output":1,"totalTokens":11,"cost":{"total":0.0001}},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"a"}}"#,
        &mut f,
    );
    fold_line(
        r#"{"type":"message_update","usage":{"input":10,"output":3,"totalTokens":13,"cost":{"total":0.0003}},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"b"}}"#,
        &mut f,
    );
    let partial = f.preview();
    assert_eq!(partial.model_calls.len(), 1);
    assert_eq!(partial.model_calls[0].total_tokens, Some(13));
    assert_eq!(partial.model_calls[0].estimated_cost_usd_micros, Some(300));
    fold_line(
        r#"{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"ab"}]}}"#,
        &mut f,
    );
    let complete = f.preview();
    assert_eq!(complete.model_calls.len(), 1);
    assert_eq!(complete.model_calls[0].output_tokens, Some(3));
    assert_eq!(complete.model_calls[0].total_tokens, Some(13));
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
    assert_eq!(fold.last_stop_reason.as_deref(), Some("stop"));
    for last in [
        serde_json::json!({"role":"toolResult","content":"tool output"}),
        serde_json::json!({"role":"assistant","stopReason":"toolUse","content":[{"type":"text","text":"not final"}]}),
    ] {
        let mut fold = EventFold::default();
        fold_line(&serde_json::json!({"type":"agent_end","messages":[{"role":"assistant","content":"old summary"},last]}).to_string(), &mut fold);
        assert!(fold.final_assistant_text.is_empty());
        if fold.last_stop_reason.as_deref() == Some("toolUse") {
            assert_eq!(fold.tool_executions, 0);
        }
    }
}

#[test]
fn records_safe_stop_reason_and_tool_execution_count_for_diagnostics() {
    let mut fold = EventFold::default();
    fold_line(
        r#"{"type":"tool_execution_start","toolName":"read","toolCallId":"1","args":{"path":"secret-project-file"}}"#,
        &mut fold,
    );
    fold_line(
        r#"{"type":"message_end","message":{"role":"assistant","stopReason":"toolUse","content":[]}}"#,
        &mut fold,
    );
    assert_eq!(fold.tool_executions, 1);
    assert_eq!(fold.last_stop_reason.as_deref(), Some("toolUse"));
}

#[test]
fn classifies_provider_errors_without_retaining_private_error_text() {
    let mut fold = EventFold::default();
    fold_line(
        r#"{"type":"agent_end","messages":[{"role":"assistant","stopReason":"error","errorMessage":"Maximum context length exceeded for request with private prompt contents"}]}"#,
        &mut fold,
    );
    assert_eq!(fold.last_error_class, Some("context_limit"));
    assert_eq!(fold.last_stop_reason.as_deref(), Some("error"));
    assert!(!format!("{fold:?}").contains("private prompt"));
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

#[test]
fn checklist_markers_update_live_snapshots_and_old_snapshots_cannot_revert_them() {
    let mut fold = EventFold::default();
    delta(
        &mut fold,
        "text",
        0,
        "Progress update.\n<!-- koolade-checklist: 0,2 -->",
    );
    let first = fold.preview();
    assert_eq!(first.checklist, [0, 2]);
    assert_eq!(first.checklist_revision, 1);
    assert!(!first.response.contains("koolade-checklist"));

    delta(&mut fold, "text", 0, "\n<!-- koolade-checklist: 2 -->");
    let newer = fold.preview();
    assert_eq!(newer.checklist, [2]);
    assert_eq!(newer.checklist_revision, 2);
    assert!(!newer.response.contains("koolade-checklist"));

    let mut display = newer;
    display.update(first);
    assert_eq!(display.checklist, [2]);
    assert_eq!(display.checklist_revision, 2);

    delta(&mut fold, "text", 0, "\n<!-- koolade-checklist: -->");
    let cleared = fold.preview();
    assert!(cleared.checklist.is_empty());
    assert_eq!(cleared.checklist_revision, 3);
    display.update(cleared);
    assert!(display.checklist.is_empty());
}
