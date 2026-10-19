//! Exercise supervised discovery through every production provider adapter. The shared runner
//! tests prove process-tree mechanics; this matrix proves each provider actually uses that
//! boundary. Each test states its invariant and setup; the matrix uses only a Unix shell,
//! `sleep`, `touch`, `yes`, `head`, and temporary files, with low setup cost.

use super::{PROVIDERS, discover_provider};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

fn fixture(
    script: &str,
) -> (
    std::path::PathBuf,
    crate::test_support::ProcessEnvironmentGuard,
) {
    let root = std::env::temp_dir().join(format!(
        "koolade-provider-process-matrix-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("state").as_os_str());
    environment.set("XDG_CONFIG_HOME", root.join("config").as_os_str());
    environment.remove("XDG_CONFIG_DIRS");
    environment.set("PI_CODING_AGENT_DIR", root.join("pi").as_os_str());
    for id in PROVIDERS {
        let name = match id {
            "pi" => "pi",
            "codex" => "codex",
            "claude" => "claude",
            "opencode" => "opencode",
            "copilot" => "copilot",
            "antigravity" => "agy",
            _ => unreachable!("the provider list is fixed"),
        };
        let executable = root.join(name);
        write_executable(&executable, script);
        environment.set(binary_environment(id), executable.as_os_str());
    }
    (root, environment)
}

fn binary_environment(provider: &str) -> &'static str {
    match provider {
        "pi" => crate::harness::pi_harness::PI_BINARY_ENV,
        "codex" => crate::harness::codex_harness::CODEX_BINARY_ENV,
        "claude" => crate::harness::claude_harness::CLAUDE_BINARY_ENV,
        "opencode" => crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        "copilot" => crate::harness::copilot_harness::COPILOT_BINARY_ENV,
        "antigravity" => crate::harness::antigravity_harness::ANTIGRAVITY_BINARY_ENV,
        _ => unreachable!("the provider list is fixed"),
    }
}

fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

/// Production boundary: explicit binary selection through each provider adapter. A validator
/// unit test cannot prove all six adapters avoid PATH fallback; requires POSIX shell and temp
/// files only, with low setup cost (six short probes).
#[test]
fn every_provider_fails_closed_for_an_invalid_explicit_executable_path() {
    let (root, mut environment) = fixture("exit 0");
    for provider in PROVIDERS {
        let invalid = root.join(format!("missing-{provider}"));
        environment.set(binary_environment(provider), invalid.as_os_str());
    }
    for provider in PROVIDERS {
        let report = discover_provider(provider, true);
        assert_eq!(report.id, provider);
        assert!(!report.ready, "{provider} must not fall through to PATH");
        assert!(report.executable.is_none());
        assert!(
            !report.status.contains("missing-"),
            "{provider} status does not expose the explicit path"
        );
        let result = super::super::super::probe_result::from_harness_probe(&report, 1, 10, 1);
        assert_eq!(
            result.failure_stage,
            Some(crate::harness::discovery::ProbeKind::Executable)
        );
        assert!(
            report
                .diagnostic
                .as_ref()
                .is_some_and(|text| !text.is_empty()),
            "{provider} identifies the invalid executable stage"
        );
        assert!(
            report
                .diagnostic
                .as_ref()
                .is_none_or(|text| !text.contains("missing-")),
            "{provider} does not expose the explicit path"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

/// Production boundary: child exit status through all six adapters. A parser unit test cannot
/// prove signal termination maps to a bounded provider result; requires POSIX shell builtins and
/// temporary files, with low setup cost (six short probes).
#[test]
fn every_provider_maps_a_sigsegv_to_its_own_bounded_failure() {
    let (root, _environment) = fixture("ulimit -c 0; kill -SEGV $$");
    for provider in PROVIDERS {
        let report = discover_provider(provider, true);
        assert_eq!(report.id, provider);
        assert!(!report.ready, "{provider} must not treat SIGSEGV as ready");
        assert!(report.status.len() < 512, "{provider} status is bounded");
        let result = super::super::super::probe_result::from_harness_probe(&report, 2, 10, 1);
        assert!(
            result.failure_stage.is_some(),
            "{provider} failure is staged"
        );
        assert!(
            report
                .diagnostic
                .as_ref()
                .is_none_or(|text| text.len() < 2048),
            "{provider} diagnostic is bounded"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

/// Production boundary: provider output capture through the persistable report. A capture-helper
/// unit test cannot prove all adapters retain bounded fields; requires POSIX `yes` and `head`,
/// shell, and temporary files, with low setup cost (six bounded probes).
#[test]
fn every_provider_bounds_giant_cli_output_before_persistable_results() {
    let (root, _environment) = fixture("yes x | head -c 2097152");
    for provider in PROVIDERS {
        let report = discover_provider(provider, true);
        assert_eq!(report.id, provider);
        assert!(report.status.len() < 512, "{provider} status is bounded");
        assert!(report.models.iter().all(|model| model.len() <= 256));
        assert!(
            report
                .diagnostic
                .as_ref()
                .is_none_or(|text| text.len() < 2048),
            "{provider} diagnostic is bounded"
        );
        let result = super::super::super::probe_result::from_harness_probe(&report, 4, 10, 1);
        assert!(result.display_status.len() < 512);
        assert!(result.models.iter().all(|model| model.len() <= 256));
    }
    fs::remove_dir_all(root).unwrap();
}

/// Production boundary: cancellation through provider adapters into the child process group. A
/// token unit test cannot prove descendants holding stdout are killed; requires POSIX shell and
/// `sleep`/`touch`, with low setup cost (six short-lived process trees).
#[test]
fn cancellation_terminates_midstream_cli_for_every_provider() {
    let root = std::env::temp_dir().join(format!(
        "koolade-provider-cancel-matrix-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let script = root.join("probe.sh");
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("state").as_os_str());
    environment.set("XDG_CONFIG_HOME", root.join("config").as_os_str());
    environment.remove("XDG_CONFIG_DIRS");
    environment.set("PI_CODING_AGENT_DIR", root.join("pi").as_os_str());

    for provider in PROVIDERS {
        let started = root.join(format!("{provider}-started"));
        let child_started = root.join(format!("{provider}-child-started"));
        let completed = root.join(format!("{provider}-completed"));
        write_executable(
            &script,
            &format!(
                "(sleep 0.1; touch '{}'; sleep 1; touch '{}') &\nwhile [ ! -f '{}' ]; do sleep 0.01; done\ntouch '{}'; wait",
                child_started.display(),
                completed.display(),
                child_started.display(),
                started.display(),
            ),
        );
        environment.set(binary_environment(provider), script.as_os_str());

        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let started_at = Instant::now();
            let report = crate::harness::discovery::with_cancellation(worker_cancel, || {
                discover_provider(provider, true)
            });
            let _ = tx.send((report, started_at.elapsed()));
        });
        let wait_started = Instant::now();
        while (!child_started.exists() || !started.exists())
            && wait_started.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            child_started.exists() && started.exists(),
            "{provider} fake CLI and inherited-pipe child both started"
        );
        cancel.store(true, Ordering::Release);
        let (report, elapsed) = rx
            .recv_timeout(Duration::from_secs(3))
            .unwrap_or_else(|_| panic!("{provider} cancellation did not finish promptly"));
        worker
            .join()
            .expect("provider worker exits after cancellation");
        assert!(
            elapsed < Duration::from_secs(3),
            "{provider} cancelled promptly"
        );
        assert_eq!(report.id, provider);
        std::thread::sleep(Duration::from_millis(1_100));
        assert!(
            !completed.exists(),
            "{provider} child process tree must not survive cancellation"
        );
        fs::remove_file(started).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}
