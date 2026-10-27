use super::super::{ClaudeEvent, command, parse_event, prompt_input};
use crate::harness::{ExecutionMode, PlanningRequest};
use std::sync::mpsc;

#[test]
fn parses_claude_stream_events_and_usage() {
    assert_eq!(
        parse_event(r#"{"type":"system","subtype":"init","model":"sonnet-5"}"#),
        Some(ClaudeEvent::Usage {
            input: None,
            output: None,
            cost: None,
            model: Some("sonnet-5".into())
        })
    );
    assert_eq!(
        parse_event(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#
        ),
        Some(ClaudeEvent::Tool("Read".into()))
    );
    assert_eq!(
        parse_event(
            r#"{"type":"result","subtype":"success","result":"done","usage":{"input_tokens":8,"output_tokens":2},"total_cost_usd":0.01}"#
        ),
        Some(ClaudeEvent::Completed {
            text: "done".into(),
            input: Some(8),
            output: Some(2),
            cost: Some(10_000)
        })
    );
    assert_eq!(
        parse_event(r#"{"type":"result","is_error":true,"result":"not authenticated"}"#),
        Some(ClaudeEvent::Failure("not authenticated".into()))
    );
    assert_eq!(parse_event("not json"), None);
}

#[test]
fn structured_prompt_uses_stream_json_input_without_arguments() {
    let value: serde_json::Value =
        serde_json::from_str(prompt_input("private task prompt").trim()).unwrap();
    assert_eq!(
        value["message"]["content"][0]["text"],
        "private task prompt"
    );
    let request = super::support::request(std::env::temp_dir(), mpsc::channel().0);
    let args = command(std::path::Path::new("claude"), &request);
    assert!(args.iter().any(|arg| arg == "--restricted"));
    assert!(args.iter().any(|arg| arg == "--no-session-persistence"));
    assert!(args.iter().any(|arg| arg == "--permission-prompts"));
    assert!(args.iter().any(|arg| arg == "Read,Glob,Grep,Edit,Write"));
    assert!(!args.iter().any(|arg| arg == "private task prompt"));
    let read_only = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Planning,
        task_id: None,
        ..request
    };
    let args = command(std::path::Path::new("claude"), &read_only);
    assert!(args.iter().any(|arg| arg == "Read,Glob,Grep"));
}
