use super::*;

#[test]
fn tool_harness_receives_supervisor_identity_and_process_ownership_rules() {
    use std::os::unix::fs::PermissionsExt;
    use std::{
        fs,
        sync::{Arc, atomic::AtomicBool, mpsc},
    };
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let root = std::env::temp_dir().join(format!(
        "koolade-ownership-prompt-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    let pi_directory = root.join("pi-agent-source");
    fs::create_dir_all(&pi_directory).unwrap();
    fs::write(
        pi_directory.join("settings.json"),
        r#"{"defaultProvider":"fixture","defaultModel":"fixture-model"}"#,
    )
    .unwrap();
    fs::write(
        pi_directory.join("models.json"),
        r#"{"providers":{"fixture":{"api":"openai-completions","baseUrl":"https://api.example.invalid/v1","models":[{"id":"fixture-model","name":"Fixture Model"}]}}}"#,
    )
    .unwrap();
    let script = root.join("pi");
    fs::write(&script, r#"#!/bin/sh
if [ "$1" = --help ]; then
    printf '%s\n' '--print' '--mode <mode> json' '--provider' '--model' '--no-session' '--no-approve' '--append-system-prompt' '--thinking <level> xhigh' '--no-extensions' '--no-skills' '--no-prompt-templates' '--no-themes' '--no-context-files' '--no-tools' '--tools' '--no-builtin-tools' '--extension'
    exit 0
fi
args="$*"
system=''
while [ "$#" -gt 0 ]; do
    if [ "$1" = --append-system-prompt ]; then
        shift
        system=$1
    fi
    shift
done
cat >/dev/null
case "$system" in *"Custom persona remains intact"*) persona=yes ;; *) persona=no ;; esac
case "$system" in *"PID "*) supervisor=yes ;; *) supervisor=no ;; esac
case "$system" in *"Do not use pkill/killall"*) ownership=yes ;; *) ownership=no ;; esac
case "$system" in *"unique inherited run marker"*) marker=yes ;; *) marker=no ;; esac
case "$system" in *"Repository reads are available only through Kool.ad/e's read-only MCP tool"*) policy=yes ;; *) policy=no ;; esac
case "$args" in *"read,grep,find,ls"*) readonly_tools=yes ;; *) readonly_tools=no ;; esac
printf '{"type":"agent_end","messages":[{"role":"assistant","content":[{"type":"text","text":"fixture completed|%s|%s|%s|%s|%s|%s"}]}]}\n' "$persona" "$supervisor" "$ownership" "$marker" "$policy" "$readonly_tools"
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    environment.set(PI_BINARY_ENV, &script);
    environment.set("PI_CODING_AGENT_DIR", &pi_directory);
    let outcomes = [
        crate::harness::ExecutionMode::Planning,
        crate::harness::ExecutionMode::TaskGeneration,
        crate::harness::ExecutionMode::Investigation,
    ]
    .into_iter()
    .map(|mode| {
        let (progress_tx, _rx) = mpsc::channel();
        PiHarness.execute(&PlanningRequest {
            prompt_manifest: None,
            mode,
            task_id: None,
            reasoning_level: "low".into(),
            telemetry_phase: None,
            repo_root: root.clone(),
            runtime_config_source: None,
            prompt_body: "test".into(),
            system_instructions: "Custom persona remains intact".into(),
            timeout: Duration::from_secs(5),
            progress_tx,
            cancel: Arc::new(AtomicBool::new(false)),
            execution_workspace: None,

            authorization_verifier: None,
            review_identity: None,
        })
    })
    .collect::<Result<Vec<_>, _>>();
    let outcomes = outcomes.unwrap();
    assert_eq!(outcomes.len(), 3);
    let expected_policy = "no";
    let expected_tools = "no";
    assert!(
        outcomes.iter().all(|outcome| outcome.final_text
            == format!("fixture completed|yes|yes|yes|yes|{expected_policy}|{expected_tools}")),
        "probe fixture returned: {:?}",
        outcomes
            .iter()
            .map(|outcome| &outcome.final_text)
            .collect::<Vec<_>>()
    );
    fs::remove_dir_all(root).unwrap();
}
