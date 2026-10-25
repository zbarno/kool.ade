use super::*;
use std::time::Duration;

const TEST_MODE: &str = "KOOLADE_REMEDIATION_TEST_MODE";

// Production boundary: this re-executes the actual supervisor against a child
// process to cover pipe draining, output bounds, environment clearing, timeout,
// and process-tree cleanup. Unit fakes cannot prove OS process containment.
// It needs no Docker, SSH, or network and takes under two seconds.
#[test]
#[cfg(target_os = "linux")]
fn bounded_runner_isolates_environment_and_redacts_limited_output() {
    if std::env::var(TEST_MODE).as_deref() == Ok("output") {
        let names = std::env::vars_os()
            .map(|(name, _)| name.to_string_lossy().to_ascii_uppercase())
            .collect::<Vec<_>>();
        assert!(!names.iter().any(|name| name.contains("AWS_")));
        assert!(!names.iter().any(|name| name.contains("RUSTFLAGS")));
        print!("https://synthetic-secret@example.invalid");
        println!("\nclientSecret: synthetic-process-secret with spaces");
        println!("GITHUB_TOKEN=synthetic-github-token");
        println!("OPENAI_API_KEY: \"synthetic-openai-key\"");
        println!("DB_PASSWORD: synthetic-database-password with spaces");
        println!("DB_PASSWORD=synthetic-process-equals first-word synthetic-process-equals-tail");
        println!("password: 'can''t reveal synthetic-process-single-quote-secret'");
        println!("https://storage.invalid/blob?sig=synthetic-process-sas");
        println!(
            "https://s3.invalid/object?X-Amz-Signature=synthetic-process-aws&X-Amz-Credential=synthetic-process-access-key"
        );
        print!("{}", "x".repeat(128 * 1024));
        return;
    }
    let name = std::thread::current().name().unwrap().to_owned();
    let invocation = TrustedInvocation::test_helper(&name, "output", Duration::from_secs(5));
    let output = SystemCommandRunner
        .run(invocation, &Cancellation::new())
        .unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout_truncated);
    assert!(output.stdout.len() <= OUTPUT_LIMIT);
    assert!(!output.stdout.contains("synthetic-secret"));
    assert!(!output.stdout.contains("synthetic-process-secret"));
    assert!(!output.stdout.contains("synthetic-github-token"));
    assert!(!output.stdout.contains("synthetic-openai-key"));
    assert!(!output.stdout.contains("synthetic-database-password"));
    assert!(!output.stdout.contains("synthetic-process-equals"));
    assert!(!output.stdout.contains("first-word"));
    assert!(
        !output
            .stdout
            .contains("synthetic-process-single-quote-secret")
    );
    assert!(!output.stdout.contains("synthetic-process-sas"));
    assert!(!output.stdout.contains("synthetic-process-aws"));
    assert!(!output.stdout.contains("synthetic-process-access-key"));
    assert!(!output.stdout.contains("with spaces"));
}

// Production boundary: the supervisor must own descendants after a timeout,
// including a child that outlives its launcher. A mock runner cannot prove this.
// This starts only the current test binary and has no external setup cost.
#[test]
#[cfg(target_os = "linux")]
fn timeout_terminates_descendant_processes() {
    if let Ok(mode) = std::env::var(TEST_MODE) {
        if let Some(marker) = mode.strip_prefix("descendant|") {
            std::thread::sleep(Duration::from_millis(600));
            std::fs::write(marker, "survived").unwrap();
            return;
        }
        if let Some(parameters) = mode.strip_prefix("spawn|") {
            let (marker, blocked_marker) = parameters.split_once('|').unwrap();
            let name = std::thread::current().name().unwrap().to_owned();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", &name, "--nocapture"])
                .env(TEST_MODE, format!("descendant|{marker}"));
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::process::CommandExt;
                unsafe {
                    command.pre_exec(|| {
                        if libc::setsid() == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
            match command.spawn() {
                Ok(mut child) => {
                    let _ = child.try_wait();
                }
                Err(_) => std::fs::write(blocked_marker, "session escape blocked").unwrap(),
            }
            std::thread::sleep(Duration::from_secs(10));
            return;
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("descendant-survived");
    let blocked_marker = directory.path().join("session-escape-blocked");
    let mode = format!("spawn|{}|{}", marker.display(), blocked_marker.display());
    let name = std::thread::current().name().unwrap().to_owned();
    let invocation = TrustedInvocation::test_helper(&name, &mode, Duration::from_millis(250));
    let result = SystemCommandRunner.run(invocation, &Cancellation::new());
    assert!(result.is_err());
    std::thread::sleep(Duration::from_millis(700));
    assert!(
        !marker.exists(),
        "remediation descendant survived timeout cleanup"
    );
    #[cfg(target_os = "linux")]
    assert_eq!(
        std::fs::read_to_string(blocked_marker).unwrap(),
        "session escape blocked"
    );
}

// Production boundary: cancellation must terminate the real child process;
// a fake runner cannot prove this. It launches only the test binary and needs
// no Docker, SSH, network, or privileged service.
#[test]
#[cfg(target_os = "linux")]
fn explicit_cancellation_stops_the_supervised_process() {
    if std::env::var(TEST_MODE).as_deref() == Ok("sleep") {
        std::thread::sleep(Duration::from_secs(10));
        return;
    }
    let name = std::thread::current().name().unwrap().to_owned();
    let invocation = TrustedInvocation::test_helper(&name, "sleep", Duration::from_secs(8));
    let cancellation = std::sync::Arc::new(Cancellation::new());
    let worker_cancellation = cancellation.clone();
    let worker =
        std::thread::spawn(move || SystemCommandRunner.run(invocation, &worker_cancellation));
    std::thread::sleep(Duration::from_millis(100));
    cancellation.cancel();
    let result = worker.join().unwrap();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

#[test]
fn typed_program_rejects_cross_platform_invocation() {
    assert!(
        TrustedInvocation::new(
            crate::app::bootstrap_repair_policy::Host::Windows,
            TrustedProgram::AptGet,
            Vec::new(),
            Duration::from_secs(5),
        )
        .is_err()
    );
}
