use super::*;
use crate::harness::{ExecutionMode, PlanningRequest};
use std::sync::{Arc, atomic::AtomicBool, mpsc};
use std::time::Duration;

#[test]
fn rendered_role_messages_survive_all_six_provider_adapters() {
    let rendered = render(&base(
        PromptRole::Implementer,
        RolePayloadV1::Worker {
            assigned_task: "Implement the approved task".into(),
            assigned_subtasks: vec![],
            immutable_baseline: "baseline-1".into(),
            allowed_execution_tools: vec!["read".into(), "edit".into()],
        },
    ))
    .unwrap();
    let request = PlanningRequest {
        prompt_manifest: Some(rendered.manifest.clone()),
        mode: ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: std::env::temp_dir(),
        runtime_config_source: None,
        prompt_body: rendered.user.clone(),
        system_instructions: rendered.system.clone(),
        timeout: Duration::from_secs(2),
        progress_tx: mpsc::channel().0,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    };

    let (codex_args, codex_user) = crate::harness::codex_harness::command(
        std::path::Path::new("codex"),
        &request,
        None,
        std::path::Path::new("/private/review"),
    );
    let codex_system = codex_args
        .iter()
        .find_map(|arg| arg.strip_prefix("developer_instructions="))
        .map(|encoded| serde_json::from_str::<String>(encoded).unwrap())
        .unwrap();
    assert_eq!(codex_user, rendered.user);
    assert!(codex_system.contains(&rendered.system));

    let claude_system =
        crate::harness::claude_harness::developer_instructions(&request, "boundary");
    let claude_user: serde_json::Value =
        serde_json::from_str(crate::harness::claude_harness::prompt_input(&rendered.user).trim())
            .unwrap();
    assert!(claude_system.contains(&rendered.system));
    assert_eq!(
        claude_user["message"]["content"][0]["text"].as_str(),
        Some(rendered.user.as_str())
    );

    for (provider, prompt) in [
        (
            "OpenCode",
            crate::harness::opencode_harness::prompt_input(&request, "boundary"),
        ),
        (
            "Copilot",
            crate::harness::copilot_harness::prompt_input(&request, "boundary"),
        ),
    ] {
        assert!(
            prompt.contains(&rendered.system),
            "{provider} system prompt"
        );
        assert!(prompt.contains(&rendered.user), "{provider} user prompt");
    }

    let antigravity: serde_json::Value = serde_json::from_str(
        &crate::harness::antigravity_harness::prompt_input_with_policy(&request, "boundary"),
    )
    .unwrap();
    let antigravity_message = antigravity["message"]["content"].as_str().unwrap();
    assert!(antigravity_message.contains(&rendered.system));
    assert!(antigravity_message.contains(&rendered.user));

    let pi_system = crate::harness::pi_harness::system_instructions(&request, "boundary");
    assert!(pi_system.contains(&rendered.system));
    assert_eq!(
        crate::harness::pi_harness::user_input(&request),
        rendered.user
    );
}
