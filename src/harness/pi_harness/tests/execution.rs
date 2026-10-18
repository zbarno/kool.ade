use super::super::*;

#[cfg(unix)]
mod supervisor;

#[cfg(unix)]
#[test]
fn implementation_harness_without_an_application_session_fails_before_provider_launch() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, atomic::AtomicBool, mpsc};
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION")
            .ok()
            .as_deref()
            != Some("1")
    {
        eprintln!(
            "skipping Docker-backed provider integration; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1 to opt in"
        );
        return;
    }
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("docker-harness-argv");
    let root = std::env::temp_dir().join(format!(
        "koolade-docker-harness-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", root.join("koolade-home"));
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
    fs::write(
        pi_directory.join("auth.json"),
        r#"{"fixture":{"key":"synthetic-auth"}}"#,
    )
    .unwrap();
    environment.set("PI_CODING_AGENT_DIR", &pi_directory);
    let (progress_tx, _rx) = mpsc::channel();
    let repository = root.join("repository");
    fs::create_dir(&repository).unwrap();
    let init = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&repository)
        .status()
        .unwrap();
    assert!(init.success());
    fs::write(repository.join("tracked.txt"), "initial\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Koolade test",
                "-c",
                "user.email=koolade-test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "initial",
            ])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    let repo = crate::persistence::state_root()
        .join("projects")
        .join("sandbox-test-project")
        .join("task-repositories")
        .join(crate::persistence::project_slug(&repository))
        .join(format!(
            "implementation-task-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..16]
        ));
    fs::create_dir_all(repo.parent().unwrap()).unwrap();
    assert!(
        std::process::Command::new("git")
            .args([
                "clone",
                "--quiet",
                "--no-hardlinks",
                repository.to_str().unwrap(),
                repo.to_str().unwrap(),
            ])
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    for (key, value) in [
        ("user.name", "Koolade test"),
        ("user.email", "koolade-test@example.invalid"),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(["config", key, value])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(
        std::process::Command::new("git")
            .args(["switch", "--quiet", "-c", "koolade-docker-harness"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir_all(repo.join(".pi/extensions")).unwrap();
    fs::write(
        repo.join("AGENTS.md"),
        "malicious project startup instructions must not reach the controller\n",
    )
    .unwrap();
    fs::write(
        repo.join(".pi/extensions/startup.ts"),
        "throw new Error('project startup hook executed on the host');\n",
    )
    .unwrap();
    let script = root.join("pi");
    fs::write(
            &script,
            r##"#!/bin/sh
if [ "$1" = --help ]; then
    printf '%s\n' '--print' '--mode <mode> json' '--provider' '--model' '--no-session' '--no-approve' '--append-system-prompt' '--thinking <level> xhigh' '--no-extensions' '--no-skills' '--no-prompt-templates' '--no-themes' '--no-context-files' '--no-tools' '--tools' '--no-builtin-tools' '--extension'
    exit 0
fi
dir=$(dirname "$0")
pwd > "$dir/controller-cwd.txt"
printf '%s' "$PI_CODING_AGENT_DIR" > "$dir/pi-agent-dir.txt"
printf '%s\n%s' "$PI_CODING_AGENT_SESSION_DIR" "$PI_PACKAGE_DIR" > "$dir/pi-isolated-dirs.txt"
printf '%s' "$PI_OFFLINE:$PI_SKIP_VERSION_CHECK" > "$dir/startup-network-flags.txt"
cp "$PI_CODING_AGENT_DIR/settings.json" "$dir/isolated-settings.json"
printf '%s\n' "$@" > "$dir/received-args.txt"
if [ -f AGENTS.md ] || [ -f .pi/extensions/startup.ts ]; then
    printf '%s\n' 'project startup input was visible from the controller cwd' > "$dir/project-startup-input-visible.txt"
fi
previous=''
for arg in "$@"; do
    if [ "$previous" = --extension ]; then cp "$arg" "$dir/extension.ts"; fi
    previous=$arg
done
cat >/dev/null
printf '%s\n' '{"type":"agent_end","messages":[{"role":"assistant","content":[{"type":"text","text":"bounded fixture completed"}]}]}'
"##,
        )
        .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    environment.set(PI_BINARY_ENV, &script);
    let outcome = PiHarness.execute(&PlanningRequest {
        prompt_manifest: None,
        mode: crate::harness::ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "low".into(),
        telemetry_phase: None,
        repo_root: repo.clone(),
        runtime_config_source: None,
        prompt_body: "test".into(),
        system_instructions: "implementation persona".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,

        authorization_verifier: None,
        review_identity: None,
    });
    let error = outcome.expect_err("an implementation without an app-owned workspace fails closed");
    assert!(
        error
            .to_string()
            .contains("application-selected task workspace")
    );
    assert!(
        !root.join("received-args.txt").exists(),
        "Pi must not launch when Kool.ad/e has not selected an owned task workspace"
    );
    fs::remove_dir_all(root).unwrap();
}
