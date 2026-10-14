use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use super::*;

fn script(body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("koolade-probe-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let path = root.join("probe.sh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    (root, path)
}

fn run_script(
    path: &std::path::Path,
    timeout: Duration,
    limit: usize,
) -> anyhow::Result<ProbeOutput> {
    run(
        &[path.to_string_lossy().into_owned()],
        timeout,
        limit,
        limit,
        &[],
    )
}

#[test]
fn output_is_bounded_independently_for_stdout_and_stderr() {
    let (root, path) = script(
        "i=0; while [ $i -lt 2000 ]; do echo 1234567890; echo abcdefghij >&2; i=$((i+1)); done",
    );
    let result = run_script(&path, Duration::from_secs(2), 1024).unwrap();
    assert_eq!(result.stdout.len(), 1024);
    assert_eq!(result.stderr.len(), 1024);
    assert!(result.stdout_truncated && result.stderr_truncated);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hung_probe_is_terminated_at_its_deadline() {
    let (root, path) = script("sleep 5");
    let result = run_script(&path, Duration::from_millis(50), 1024);
    assert!(result.unwrap_err().to_string().contains("deadline"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn crashed_probe_is_reported_as_a_provider_failure() {
    let (root, path) = script("ulimit -c 0; kill -SEGV $$");
    let result = run_script(&path, Duration::from_secs(2), 1024).unwrap();
    assert!(!result.success);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancellation_terminates_a_running_probe_before_its_deadline() {
    let (root, path) = script("sleep 30");
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = cancellation.clone();
    let started = Instant::now();
    let worker = thread::spawn(move || {
        with_cancellation(worker_cancellation, || {
            run_script(&path, Duration::from_secs(10), 1024)
        })
    });
    thread::sleep(Duration::from_millis(80));
    cancellation.store(true, Ordering::Release);
    let result = worker.join().unwrap();
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(started.elapsed() < Duration::from_secs(2));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn descendant_holding_pipe_is_killed_without_an_unbounded_join() {
    let (root, path) = script("");
    let marker = root.join("descendant-survived");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\n(sleep 1; printf leaked > '{}') &\necho finished\n",
            marker.display()
        ),
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run_script(&path, Duration::from_secs(2), 1024));
    });
    let result = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("probe must not wait indefinitely for a descendant");
    let result = result.unwrap();
    assert!(String::from_utf8_lossy(&result.stdout).contains("finished"));
    thread::sleep(Duration::from_millis(1100));
    assert!(
        !marker.exists(),
        "a background descendant must not survive the completed probe"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn timeout_terminates_a_forked_process_tree_with_a_bounded_wait() {
    let (root, path) = script("");
    let marker = root.join("timed-out-descendant-survived");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\n(sleep 1; printf leaked > '{}') &\nsleep 30\n",
            marker.display()
        ),
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run_script(&path, Duration::from_millis(100), 1024));
    });
    let result = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("timed-out probe must reap its process tree promptly");
    assert!(result.unwrap_err().to_string().contains("deadline"));
    thread::sleep(Duration::from_millis(1100));
    assert!(
        !marker.exists(),
        "a background descendant must not survive a timed-out probe"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_runs_from_a_private_directory_outside_the_checkout() {
    let (root, path) = script(
        "if [ -f AGENTS.md ] || [ -d .pi/extensions ]; then echo project-config-visible; else echo isolated; fi; pwd",
    );
    fs::write(root.join("AGENTS.md"), "Hostile repository instructions").unwrap();
    fs::create_dir_all(root.join(".pi/extensions")).unwrap();
    fs::write(
        root.join(".pi/extensions/startup.js"),
        "throw new Error('loaded')",
    )
    .unwrap();
    let result = run_script(&path, Duration::from_secs(2), 1024).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    let mut lines = output.lines();
    assert_eq!(lines.next(), Some("isolated"));
    let working_directory = lines.next().unwrap_or_default().trim();
    assert_ne!(
        std::path::Path::new(&working_directory),
        std::env::current_dir().unwrap()
    );
    assert!(!working_directory.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_code_environment_is_removed_before_provider_probe() {
    let (root, path) = script("printf '%s' \"${NODE_OPTIONS-unset}\"");
    let output = run_inner(
        &[path.to_string_lossy().into_owned()],
        Duration::from_secs(2),
        1024,
        1024,
        &[],
        &[("NODE_OPTIONS", "--require=/tmp/host-injected.js")],
    )
    .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "unset");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_and_project_injection_variables_are_removed_before_every_probe() {
    let (root, path) = script("env");
    let hostile_environment = crate::harness::execution_security::UNSAFE_PROVIDER_LAUNCH_ENV
        .iter()
        .map(|name| (*name, "synthetic-hostile-value"))
        .collect::<Vec<_>>();
    let output = run_inner(
        &[path.to_string_lossy().into_owned()],
        Duration::from_secs(2),
        64 * 1024,
        64 * 1024,
        &[],
        &hostile_environment,
    )
    .unwrap();
    assert!(!output.stdout_truncated);
    let inherited = String::from_utf8_lossy(&output.stdout);
    for name in crate::harness::execution_security::UNSAFE_PROVIDER_LAUNCH_ENV {
        let prefix = format!("{name}=");
        assert!(
            !inherited
                .lines()
                .any(|entry| entry.starts_with(prefix.as_str())),
            "probe inherited unsafe variable {name}"
        );
    }
    assert!(!inherited.contains("synthetic-hostile-value"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_inherits_only_the_small_nonsecret_environment_allowlist() {
    let (root, path) = script(
        "printf '%s|%s|%s' \"${OPENAI_API_KEY-unset}\" \"${KOOLADE_PROBE_PRIVATE-unset}\" \"${PATH-unset}\"",
    );
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("OPENAI_API_KEY", "synthetic-test-value");
    environment.set("KOOLADE_PROBE_PRIVATE", "synthetic-test-value");
    let result = run_inner(
        &[path.to_string_lossy().into_owned()],
        Duration::from_secs(2),
        1024,
        1024,
        &[],
        &[("PATH", "/synthetic/test-path")],
    )
    .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "unset|unset|/synthetic/test-path"
    );
    drop(environment);
    fs::remove_dir_all(root).unwrap();
}
