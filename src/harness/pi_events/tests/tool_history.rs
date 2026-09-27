use super::super::*;
#[test]
fn tool_output_preserves_order_and_replaces_partial_snapshots() {
    let mut fold = EventFold::default();
    for event in [
        r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","delta":"Check permissions","contentIndex":0}}"#,
        r#"{"type":"tool_execution_start","toolName":"bash","toolCallId":"t1","args":{"command":"cargo test"}}"#,
        r#"{"type":"tool_execution_update","toolName":"bash","toolCallId":"t1","partialResult":{"content":[{"type":"text","text":"running"}]}}"#,
        r#"{"type":"tool_execution_end","toolName":"bash","toolCallId":"t1","result":{"content":[{"type":"text","text":"all tests passed"}]}}"#,
    ] {
        fold_line(event, &mut fold);
    }
    let p = fold.preview();
    assert_eq!(p.posts.len(), 2);
    assert_eq!(p.posts[0].kind, "thinking");
    assert!(p.posts[1].text.contains("cargo test"));
    assert!(p.posts[1].text.ends_with("all tests passed"));
    assert!(!p.posts[1].text.contains("running"));
    assert_eq!(p.thoughts, "Check permissions");
}
