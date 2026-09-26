use super::super::*;

#[cfg(unix)]
#[test]
fn tool_harness_receives_supervisor_identity_and_process_ownership_rules() {
    use std::os::unix::fs::PermissionsExt;
    use std::{
        fs,
        sync::{Arc, atomic::AtomicBool, mpsc},
    };
    let _shield = crate::core::gitops::test_support::shield("process-ownership-prompt");
    let root = std::env::temp_dir().join(format!(
        "packet-ownership-prompt-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    let script = root.join("pi");
    fs::write(&script, r#"#!/bin/sh
if [ "$1" = --help ]; then
    printf '%s\n' '--print' '--mode <mode> json' '--no-session' '--no-approve' '--append-system-prompt' '--thinking <level> xhigh' '--no-extensions' '--no-skills' '--no-prompt-templates' '--no-context-files' '--no-tools' '--tools' '--no-builtin-tools' '--extension'
    exit 0
fi
printf '%s\n' "$@" > "$(dirname "$0")/received-args.txt"
while [ "$#" -gt 0 ]; do
    if [ "$1" = --append-system-prompt ]; then
        shift
        printf '%s' "$1" > "$(dirname "$0")/received-system.txt"
    fi
    shift
done
cat >/dev/null
printf '%s\n' '{"type":"agent_end","messages":[{"role":"assistant","content":[{"type":"text","text":"fixture completed"}]}]}'
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let previous = std::env::var_os(PI_BINARY_ENV);
    unsafe {
        std::env::set_var(PI_BINARY_ENV, &script);
    }
    let (progress_tx, _rx) = mpsc::channel();
    let outcome = PiHarness.execute(&PlanningRequest {
        mode: crate::harness::ExecutionMode::Planning,
        reasoning_level: "low".into(),
        repo_root: root.clone(),
        prompt_body: "test".into(),
        system_instructions: "Custom persona remains intact".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
    });
    unsafe {
        match previous {
            Some(value) => std::env::set_var(PI_BINARY_ENV, value),
            None => std::env::remove_var(PI_BINARY_ENV),
        }
    }
    assert_eq!(outcome.unwrap().final_text, "fixture completed");
    let sent = fs::read_to_string(root.join("received-system.txt")).unwrap();
    assert!(sent.ends_with("Custom persona remains intact"));
    assert!(sent.contains(&format!("PID {}", std::process::id())));
    assert!(sent.contains("Do not use pkill/killall"));
    assert!(sent.contains("unique inherited run marker"));
    let received_args = fs::read_to_string(root.join("received-args.txt")).unwrap();
    assert!(received_args.contains("--tools\nread,grep,find,ls\n"));
    assert!(!received_args.contains("--no-tools"));
    assert!(!received_args.contains("--no-builtin-tools"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn implementation_harness_exposes_only_the_bounded_shell_extension() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, atomic::AtomicBool, mpsc};
    if !cfg!(target_os = "linux")
        || std::process::Command::new("bwrap")
            .arg("--version")
            .output()
            .is_err()
    {
        return;
    }
    let _shield = crate::core::gitops::test_support::shield("sandbox-harness-argv");
    let root = std::env::temp_dir().join(format!(
        "packet-sandbox-harness-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
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
                "user.name=Packet test",
                "-c",
                "user.email=packet-test@example.invalid",
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
    let repo = root
        .join(".packet-worktrees")
        .join(crate::persistence::project_slug(&repository))
        .join("implementation-task");
    fs::create_dir_all(repo.parent().unwrap()).unwrap();
    assert!(
        std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--quiet",
                "-b",
                "packet-sandbox-harness",
                repo.to_str().unwrap(),
            ])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    let script = root.join("pi");
    fs::write(
            &script,
            r##"#!/bin/sh
if [ "$1" = --help ]; then
    printf '%s\n' '--print' '--mode <mode> json' '--no-session' '--no-approve' '--append-system-prompt' '--thinking <level> xhigh' '--no-extensions' '--no-skills' '--no-prompt-templates' '--no-context-files' '--no-tools' '--tools' '--no-builtin-tools' '--extension'
    exit 0
fi
dir=$(dirname "$0")
printf '%s\n' "$@" > "$dir/received-args.txt"
printf '%s' "$PACKET_SANDBOX_CONFIG" > "$dir/sandbox.json"
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
    let previous = std::env::var_os(PI_BINARY_ENV);
    unsafe { std::env::set_var(PI_BINARY_ENV, &script) };
    let (progress_tx, _rx) = mpsc::channel();
    let outcome = PiHarness.execute(&PlanningRequest {
        mode: crate::harness::ExecutionMode::Implementation,
        reasoning_level: "low".into(),
        repo_root: repo.clone(),
        prompt_body: "test".into(),
        system_instructions: "implementation persona".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
    });
    unsafe {
        match previous {
            Some(value) => std::env::set_var(PI_BINARY_ENV, value),
            None => std::env::remove_var(PI_BINARY_ENV),
        }
    }
    assert_eq!(outcome.unwrap().final_text, "bounded fixture completed");
    let received_args = fs::read_to_string(root.join("received-args.txt")).unwrap();
    assert!(received_args.contains("--no-builtin-tools"));
    assert!(received_args.contains("--tools\npacket_bash\n"));
    assert!(received_args.contains("--extension\n"));
    assert!(!received_args.contains("--tools\nread,grep,find,ls\n"));
    assert!(!received_args.contains("--no-tools"));
    let extension = fs::read_to_string(root.join("extension.ts")).unwrap();
    assert!(extension.contains("name: \"packet_bash\""));
    assert!(extension.contains("child.kill(\"SIGKILL\")"));
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("sandbox.json")).unwrap()).unwrap();
    let sandbox_args = config["args"].as_array().unwrap();
    assert!(sandbox_args.iter().any(|arg| arg == "--unshare-net"));
    assert!(
        sandbox_args
            .windows(3)
            .any(|args| { args[0] == "--ro-bind" && args[1] == "/" && args[2] == "/" })
    );
    fs::remove_dir_all(root).unwrap();
}
