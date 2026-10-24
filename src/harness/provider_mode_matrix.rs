//! Adapter boundary E2E: fake CLIs exercise each production `AiHarness` process
//! path and normalized result for all six providers and eight modes. Parser-only
//! unit tests cannot prove argv construction, stdin delivery, child supervision,
//! and stream-to-outcome integration together. No Docker/network dependency;
//! setup is six temporary scripts and one short process per matrix cell.

use super::{
    AiHarness, AntigravityHarness, ClaudeHarness, CodexHarness, CopilotHarness, ExecutionMode,
    OpenCodeHarness, PiHarness, PlanningRequest,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

const MODES: [ExecutionMode; 8] = ExecutionMode::ALL;
const MATRIX_RESULT: &str = "matrix-result";

#[cfg(unix)]
#[test]
fn all_six_providers_normalize_results_for_every_execution_mode() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "koolade-provider-mode-matrix-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let pi_home = root.join("pi-config");
    pi_config(&pi_home);

    let providers = [
        ("pi", super::pi_harness::PI_BINARY_ENV, script("pi")),
        (
            "codex",
            super::codex_harness::CODEX_BINARY_ENV,
            script("codex"),
        ),
        (
            "claude",
            super::claude_harness::CLAUDE_BINARY_ENV,
            script("claude"),
        ),
        (
            "opencode",
            super::opencode_harness::OPENCODE_BINARY_ENV,
            script("opencode"),
        ),
        (
            "copilot",
            super::copilot_harness::COPILOT_BINARY_ENV,
            script("copilot"),
        ),
        (
            "antigravity",
            super::antigravity_harness::ANTIGRAVITY_BINARY_ENV,
            script("antigravity"),
        ),
    ];
    for (provider, env_key, source) in &providers {
        let binary = root.join(provider);
        fs::write(&binary, source).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        environment.set(env_key, &binary);
    }
    environment.set("PI_CODING_AGENT_DIR", &pi_home);

    for (provider, _, _) in providers {
        for mode in MODES {
            let request = request(root.clone(), mode);
            let outcome = crate::harness::with_uncontained_provider_test_execution(|| {
                execute(provider, &request)
            })
            .unwrap_or_else(|error| panic!("{provider} {mode:?}: {error}"));
            assert_eq!(outcome.final_text, MATRIX_RESULT, "{provider} {mode:?}");
            assert_reasoning_argument(provider, &root.join(format!("{provider}.argv")));
        }
    }
    for provider in [
        "pi",
        "codex",
        "claude",
        "opencode",
        "copilot",
        "antigravity",
    ] {
        let capture = root.join(format!("{provider}.argv"));
        let before = fs::read(&capture).unwrap();
        let mut invalid = request(root.clone(), ExecutionMode::Planning);
        invalid.reasoning_level = "ultramax".into();
        assert!(matches!(
            execute(provider, &invalid),
            Err(crate::error::AppError::ProviderCapabilityMismatch { .. })
        ));
        assert_eq!(
            fs::read(capture).unwrap(),
            before,
            "{provider} launched despite an unsupported effort"
        );
    }
    let codex_args = fs::read_to_string(root.join("codex.argv")).unwrap();
    assert!(
        codex_args.contains("--cd\n") && !codex_args.contains(&root.to_string_lossy().to_string()),
        "Codex screenshot reviewer received the task clone as its workspace: {codex_args}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(not(unix))]
#[test]
fn all_six_providers_normalize_results_for_every_execution_mode() {}

fn execute(
    provider: &str,
    request: &PlanningRequest,
) -> Result<super::HarnessOutcome, crate::error::AppError> {
    match provider {
        "pi" => PiHarness.execute_with_model(request, Some("model-1")),
        "codex" => CodexHarness.execute_with_model(request, Some("matrix-model")),
        "claude" => ClaudeHarness.execute_with_model(request, Some("matrix-model")),
        "opencode" => OpenCodeHarness.execute_with_model(request, Some("openai/gpt-test")),
        "copilot" => CopilotHarness.execute_with_model(request, Some("matrix-model")),
        "antigravity" => AntigravityHarness.execute_with_model(request, Some("matrix-model")),
        other => panic!("unknown matrix provider {other}"),
    }
}

fn assert_reasoning_argument(provider: &str, path: &Path) {
    let args = fs::read_to_string(path).expect("fake provider recorded invocation arguments");
    let honored = match provider {
        "pi" => args.contains("--thinking\nmedium\n"),
        "codex" => args.contains("model_reasoning_effort=\"medium\""),
        "claude" | "antigravity" | "copilot" => args.contains("--effort\nmedium\n"),
        "opencode" => args.contains("--variant\nmedium\n"),
        _ => false,
    };
    assert!(
        honored,
        "{provider} did not receive the requested medium effort: {args}"
    );
}

fn request(root: PathBuf, mode: ExecutionMode) -> PlanningRequest {
    let mut request = PlanningRequest {
        prompt_manifest: None,
        mode,
        task_id: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: root,
        runtime_config_source: None,
        prompt_body: "synthetic matrix prompt".into(),
        system_instructions: "synthetic matrix instructions".into(),
        timeout: Duration::from_secs(5),
        progress_tx: mpsc::channel().0,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    };
    if mode == ExecutionMode::RuntimeQaVisualReview {
        use crate::harness::{
            ReviewExecutionIdentity, RuntimeQaReviewScreenshot, RuntimeQaScreenshotReview,
            prompt_catalog::PromptManifest,
        };
        use sha2::Digest;
        let execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let session_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let screenshot_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(1, 1))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        request.task_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
        request.telemetry_phase = Some("runtime_qa_review".into());
        request.prompt_manifest = Some(PromptManifest {
            role_version: "runtime_qa/v3".into(),
            system_template_sha256: "a".repeat(64),
            policy_template_sha256: "b".repeat(64),
            user_template_sha256: "c".repeat(64),
            output_schema_version: "ReviewVerdictV1".into(),
            privilege_profile: "read_only; isolated_fresh_review_context".into(),
        });
        request.prompt_body = format!("review execution {execution_id}, session {session_id}");
        request.review_identity = Some(ReviewExecutionIdentity {
            execution_id,
            session_id,
            runtime_qa_screenshot_review: Some(Arc::new(RuntimeQaScreenshotReview {
                workspace_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                source_sha: "d".repeat(40),
                screenshots: vec![RuntimeQaReviewScreenshot {
                    id: screenshot_id,
                    sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
                    bytes,
                }],
                inspected_screenshot_ids: Arc::default(),
            })),
        });
    }
    request
}

fn pi_config(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("settings.json"),
        r#"{"defaultProvider":"synthetic","defaultModel":"model-1"}"#,
    )
    .unwrap();
    fs::write(path.join("models.json"), r#"{"providers":{"synthetic":{"api":"openai-completions","models":[{"id":"model-1","name":"Model 1","contextWindow":4096}]}}}"#).unwrap();
}

fn script(provider: &str) -> String {
    let version = match provider {
        "pi" => "1.2.3",
        "codex" => "codex-cli 1.2.3",
        "claude" => "Claude Code 2.1.300",
        "opencode" => "opencode 1.2.3",
        "copilot" => "GitHub Copilot CLI 1.0.82",
        "antigravity" => "agy 1.2.3",
        _ => unreachable!(),
    };
    let help = match provider {
        "pi" => {
            "--print\n--mode text json rpc\n--no-session\n--no-approve\n--append-system-prompt\n--thinking xhigh\n--provider\n--model\n--no-extensions\n--no-skills\n--no-prompt-templates\n--no-themes\n--no-context-files\n--no-tools\n--tools\n--no-builtin-tools\n--extension"
        }
        "codex" => {
            "--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check"
        }
        "copilot" => {
            "--silent --no-ask-user --no-auto-update --available-tools=TOOL... --effort --additional-mcp-config --disable-builtin-mcps --no-custom-instructions --no-remote --no-remote-export --allow-tool"
        }
        "claude" => {
            "--print --output-format --input-format --verbose --no-session-persistence --restricted --permission-prompts --tools --permission-mode --effort --strict-mcp-config --mcp-config --allowedTools"
        }
        "opencode" => "--pure run --format --auto --dir --agent --model --variant",
        _ => "help",
    };
    let result = match provider {
        "pi" => {
            r#"printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"matrix-result"}]}}'"#
        }
        "codex" => {
            r#"printf '%s\n' '{"type":"error","error":{"message":"code mode is disabled on host by policy"}}' '{"type":"item.completed","item":{"type":"agent_message","text":"matrix-result"}}' '{"type":"turn.completed"}'"#
        }
        "claude" => {
            r#"printf '%s\n' '{"type":"system","subtype":"init","model":"matrix-model"}' '{"type":"result","subtype":"success","is_error":false,"result":"matrix-result","usage":{"input_tokens":1,"output_tokens":1},"total_cost_usd":0.01}'"#
        }
        "opencode" => {
            r#"printf '%s\n' '{"type":"text","part":{"text":"matrix-result"}}' '{"type":"step_finish","part":{"reason":"stop","tokens":{"input":1,"output":1}}}'"#
        }
        "copilot" => r#"printf '%s\n' 'matrix-result'"#,
        "antigravity" => {
            r#"printf '%s\n' '{"event":"result","result":{"status":"SUCCESS","response":"matrix-result","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}'"#
        }
        _ => unreachable!(),
    };
    format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '{version}'; exit 0; fi\nif [ \"$1\" = \"--help\" ] || {{ [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--help\" ]; }}; then printf '%s\\n' '{help}'; exit 0; fi\nif [ \"$1\" = \"login\" ]; then echo 'Logged in'; exit 0; fi\nif [ \"$1\" = \"auth\" ]; then echo '{{\"authMethod\":\"oauth\"}}'; exit 0; fi\ndir=$(dirname \"$0\"); name=$(basename \"$0\"); printf '%s\\n' \"$@\" > \"$dir/$name.argv\"\ncat >/dev/null\n{result}\n",
        version = version.replace('\'', "'\\''"),
        help = help.replace('\'', "'\\''"),
    )
}
