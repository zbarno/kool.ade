use super::*;

#[test]
fn parses_completed_message_activity_and_usage_events() {
    assert_eq!(
        parse_event(r#"{"type":"item.completed","item":{"type":"agent_message","text":"final"}}"#),
        Some(CodexEvent::Message("final".into()))
    );
    assert_eq!(
        parse_event(r#"{"type":"item.started","item":{"type":"command_execution"}}"#),
        Some(CodexEvent::Activity("command_execution".into()))
    );
    assert_eq!(
        parse_event(r#"{"type":"turn.completed","usage":{"input_tokens":12,"output_tokens":7}}"#),
        Some(CodexEvent::Usage {
            call_id: None,
            input: Some(12),
            output: Some(7),
            cached_input: None,
            cache_write_input: None,
        })
    );
}

#[test]
fn live_progress_retains_harness_usage_metadata() {
    let mut progress = LiveProgress::default();
    progress.update(LiveProgress {
        model_calls: vec![crate::harness::ModelCallUsage {
            call_id: "codex:test".into(),
            provider: Some("openai".into()),
            model: Some("codex-model".into()),
            input_tokens: Some(11),
            output_tokens: Some(5),
            ..Default::default()
        }],
        ..LiveProgress::default()
    });
    assert_eq!(progress.model_calls[0].input_tokens, Some(11));
    assert_eq!(progress.model_calls[0].output_tokens, Some(5));
    assert_eq!(
        progress.model_calls[0].model.as_deref(),
        Some("codex-model")
    );
}

#[test]
fn command_uses_model_reasoning_and_operation_scoped_sandbox() {
    let (tx, _rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: std::env::temp_dir(),
        runtime_config_source: None,
        prompt_body: "do task".into(),
        system_instructions: "system".into(),
        timeout: Duration::from_secs(2),
        progress_tx: tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,

        authorization_verifier: None,
        review_identity: None,
    };
    let private_workspace = Path::new("/private/review");
    let (argv, prompt) = command(
        Path::new("codex"),
        &request,
        Some("selected-model".into()),
        private_workspace,
    );
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--model", "selected-model"])
    );
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"])
    );
    assert!(
        argv.iter()
            .any(|value| value == "model_reasoning_effort=\"high\"")
    );
    assert_eq!(prompt, "do task");
    assert!(
        argv.iter()
            .any(|value| value.contains("developer_instructions="))
    );
    assert!(argv.iter().any(|value| value == "--ephemeral"));

    let read_only = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Planning,
        task_id: None,
        ..request
    };
    let (argv, _) = command(Path::new("codex"), &read_only, None, private_workspace);
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"])
    );
    let visual_review = PlanningRequest {
        mode: ExecutionMode::RuntimeQaVisualReview,
        ..read_only
    };
    let (argv, _) = command(Path::new("codex"), &visual_review, None, private_workspace);
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--cd", "/private/review"])
    );
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"])
    );
}

#[test]
fn malformed_events_are_ignored_and_failures_are_normalized() {
    assert_eq!(parse_event("not json"), None);
    assert_eq!(parse_event("{}"), None);
    assert_eq!(
        parse_event(r#"{"type":"turn.failed","error":{"message":"login required"}}"#),
        Some(CodexEvent::Failure("login required".into()))
    );
    assert_eq!(normalize_effort("XHIGH"), "xhigh");
    assert_eq!(normalize_effort("unsupported"), "medium");
}
