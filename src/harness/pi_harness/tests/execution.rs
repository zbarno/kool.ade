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
        "koolade-ownership-prompt-{}-{}",
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
case "$system" in *"Planning reads are restricted by an operating-system sandbox"*) policy=yes ;; *) policy=no ;; esac
case "$args" in *"read,grep,find,ls"*) readonly_tools=yes ;; *) readonly_tools=no ;; esac
printf '{"type":"agent_end","messages":[{"role":"assistant","content":[{"type":"text","text":"fixture completed|%s|%s|%s|%s|%s|%s"}]}]}\n' "$persona" "$supervisor" "$ownership" "$marker" "$policy" "$readonly_tools"
"#).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let previous = std::env::var_os(PI_BINARY_ENV);
    unsafe {
        std::env::set_var(PI_BINARY_ENV, &script);
    }
    let outcomes = [
        crate::harness::ExecutionMode::Planning,
        crate::harness::ExecutionMode::TaskGeneration,
        crate::harness::ExecutionMode::Investigation,
    ]
    .into_iter()
    .map(|mode| {
        let (progress_tx, _rx) = mpsc::channel();
        PiHarness.execute(&PlanningRequest {
            mode,
            reasoning_level: "low".into(),
            model: None,
            repo_root: root.clone(),
            prompt_body: "test".into(),
            system_instructions: "Custom persona remains intact".into(),
            timeout: Duration::from_secs(5),
            progress_tx,
            cancel: Arc::new(AtomicBool::new(false)),
        })
    })
    .collect::<Result<Vec<_>, _>>();
    unsafe {
        match previous {
            Some(value) => std::env::set_var(PI_BINARY_ENV, value),
            None => std::env::remove_var(PI_BINARY_ENV),
        }
    }
    let outcomes = outcomes.unwrap();
    assert_eq!(outcomes.len(), 3);
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.final_text == "fixture completed|yes|yes|yes|yes|yes|yes")
    );
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
        "koolade-sandbox-harness-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
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
    let repo = root
        .join(".koolade-worktrees")
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
                "koolade-sandbox-harness",
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
printf '%s' "$KOOLADE_SANDBOX_CONFIG" > "$dir/sandbox.json"
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
    let outcome = PiHarness.execute(&PlanningRequest {
        mode: crate::harness::ExecutionMode::Implementation,
        reasoning_level: "low".into(),
        model: None,
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
    assert!(received_args.contains("--tools\nkoolade_bash,koolade_resource\n"));
    assert!(received_args.contains("--extension\n"));
    assert!(!received_args.contains("--tools\nread,grep,find,ls\n"));
    assert!(!received_args.contains("--no-tools"));
    let extension = fs::read_to_string(root.join("extension.ts")).unwrap();
    assert!(extension.contains("name: \"koolade_bash\""));
    assert!(extension.contains("name: \"koolade_resource\""));
    assert!(extension.contains("KOOLADE_RESOURCE_SOCKET"));
    assert!(extension.contains("prepare_nuget_audit"));
    assert!(extension.contains("could not refresh the public NuGet audit feed"));
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
