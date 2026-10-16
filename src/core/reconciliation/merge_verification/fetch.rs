use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// Fetch GitHub's stable PR head ref and require that it resolves to the exact
/// reviewed commit. This remains available when the source branch is deleted.
pub fn fetch_pull_request_head(
    repository: &Path,
    pull_request_number: u64,
    expected_head: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        pull_request_number > 0 && super::valid_oid(expected_head),
        "invalid persisted PR identity"
    );
    let refspec = format!("refs/pull/{pull_request_number}/head");
    fetch_with_deadline(repository, &["fetch", "--no-tags", "origin", &refspec])?;
    let fetched = super::git(
        repository,
        &["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
    )?;
    anyhow::ensure!(
        fetched == expected_head,
        "provider PR head ref does not match the independently reviewed commit"
    );
    Ok(())
}

pub fn fetch_origin_with_deadline(repository: &Path) -> anyhow::Result<()> {
    fetch_with_deadline(repository, &["fetch", "--no-tags", "origin"])
}

fn fetch_with_deadline(repository: &Path, args: &[&str]) -> anyhow::Result<()> {
    const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
    let mut command = crate::core::planning_sync::validated_git_command(repository)?;
    command.args(args);
    let status = run_bounded_process(command, FETCH_TIMEOUT)?;
    anyhow::ensure!(status.success(), "provider Git fetch failed");
    Ok(())
}

pub(crate) fn run_bounded_process(
    mut command: Command,
    timeout: Duration,
) -> anyhow::Result<std::process::ExitStatus> {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let mut process_tree = Some(
        crate::harness::discovery::windows::WindowsProcessTree::attach(child.id()).map_err(
            |error| {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::anyhow!("could not supervise provider Git fetch tree: {error}")
            },
        )?,
    );
    let started = Instant::now();
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                #[cfg(unix)]
                terminate_tree(&mut child)?;
                #[cfg(windows)]
                terminate_tree(&mut child, &mut process_tree)?;
                return Err(error.into());
            }
        };
        if let Some(status) = status {
            #[cfg(windows)]
            drop(process_tree.take());
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            #[cfg(unix)]
            terminate_tree(&mut child)?;
            #[cfg(windows)]
            terminate_tree(&mut child, &mut process_tree)?;
            anyhow::bail!(
                "provider Git fetch timed out after {} seconds",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn terminate_tree(child: &mut std::process::Child) -> anyhow::Result<()> {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let pid = i32::try_from(child.id())?;
    unsafe {
        kill(-pid, 9);
    }
    let _ = child.kill();
    let _ = child.wait()?;
    Ok(())
}

#[cfg(windows)]
fn terminate_tree(
    child: &mut std::process::Child,
    process_tree: &mut Option<crate::harness::discovery::windows::WindowsProcessTree>,
) -> anyhow::Result<()> {
    drop(process_tree.take());
    let _ = child.kill();
    let _ = child.wait()?;
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
#[test]
// Production boundary: the fetch process supervisor owns a real helper process
// group. A direct kill-helper unit test cannot prove descendant cleanup; local
// shell and `/proc` only, low setup cost, no network or external service.
fn timeout_terminates_the_owned_process_group() {
    let directory =
        std::env::temp_dir().join(format!("koolade-fetch-tree-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let pid_file = directory.join("child.pid");
    let script = directory.join("helper.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 60 &\necho $! > '{}'\nwait\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let error = run_bounded_process(
        {
            let mut command = Command::new("sh");
            command.arg(&script);
            command
        },
        Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(error.to_string().contains("timed out"));
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    for _ in 0..80 {
        let proc_state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(')')
                    .map(|(_, rest)| rest.trim().to_owned())
            })
            .and_then(|rest| rest.chars().next());
        if unsafe { kill(pid, 0) } != 0 || matches!(proc_state, Some('Z' | 'X')) {
            let _ = std::fs::remove_dir_all(directory);
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed-out remote helper descendant is still alive");
}

#[cfg(all(test, windows))]
#[test]
// Production boundary: the supervisor's Windows JobObject owns a child helper.
// A mocked process tree cannot prove timeout cleanup; requires local PowerShell,
// with low setup cost and no network or external service.
fn timeout_closes_the_windows_job_and_its_descendant() {
    let directory = std::env::temp_dir().join(format!(
        "koolade-fetch-job-{}",
        uuid::Uuid::new_v4().hyphenated()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let pid_file = directory.join("child.pid");
    let command = format!(
        "$p=Start-Process -FilePath powershell.exe -ArgumentList '-NoProfile -Command Start-Sleep -Seconds 60' -PassThru; Set-Content -Path '{}' -Value $p.Id; Wait-Process -Id $p.Id",
        pid_file.display()
    );
    let error = run_bounded_process(
        Command::new("powershell.exe").args(["-NoLogo", "-NoProfile", "-Command", &command]),
        Duration::from_secs(2),
    )
    .unwrap_err();
    assert!(error.to_string().contains("timed out"));
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    let still_running = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-Command",
            &format!("if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 1 }} else {{ exit 0 }}"),
        ])
        .status()
        .unwrap();
    let _ = std::fs::remove_dir_all(directory);
    assert!(
        still_running.success(),
        "job-owned fetch descendant survived timeout"
    );
}
