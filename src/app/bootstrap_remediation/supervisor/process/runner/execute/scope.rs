use std::{ffi::OsString, path::PathBuf};

use crate::app::bootstrap_remediation::supervisor::process::{
    ProcessRunError, ProcessRunFailure, program::TrustedInvocation,
};

const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const PKEXEC: &str = "/usr/bin/pkexec";

pub(super) struct ElevatedLaunch {
    pub(super) systemd_run: PathBuf,
    pub(super) arguments: Vec<OsString>,
    pub(super) unit_name: String,
}

pub(super) fn prepare(invocation: &TrustedInvocation) -> Result<ElevatedLaunch, ProcessRunError> {
    use std::time::Duration;

    let systemd_run = PathBuf::from(SYSTEMD_RUN);
    let pkexec = PathBuf::from(PKEXEC);
    let target = invocation
        .program
        .resolve(invocation.host)
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    super::super::super::platform::validate_program_path(&systemd_run)
        .and_then(|_| super::super::super::platform::validate_program_path(&target))
        .and_then(|_| validate_pkexec(&pkexec))
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    let unit_name = format!("koolade-remediation-{}.service", uuid::Uuid::new_v4());
    let mut arguments = vec![
        "--user".into(),
        format!("--unit={unit_name}").into(),
        "--wait".into(),
        "--pipe".into(),
        "--collect".into(),
        "--quiet".into(),
        "--property=Type=exec".into(),
        "--property=KillMode=control-group".into(),
        "--property=UMask=0022".into(),
        format!(
            "--property=RuntimeMaxSec={}s",
            invocation.timeout.as_secs().max(1)
        )
        .into(),
        "--property=TimeoutStopSec=5s".into(),
        "--".into(),
        pkexec.into_os_string(),
        target.into_os_string(),
    ];
    for argument in &invocation.arguments {
        argument
            .validate()
            .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        arguments.push(argument.as_os_string());
    }
    if invocation.timeout > Duration::from_secs(600) {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    }
    Ok(ElevatedLaunch {
        systemd_run,
        arguments,
        unit_name,
    })
}

pub(super) fn validate_unit_name(value: &str) -> bool {
    value
        .strip_prefix("koolade-remediation-")
        .and_then(|tail| tail.strip_suffix(".service"))
        .is_some_and(|uuid| uuid::Uuid::parse_str(uuid).is_ok())
}

pub(super) fn stop_unit(unit_name: &str) -> anyhow::Result<()> {
    use std::{
        thread,
        time::{Duration, Instant},
    };

    anyhow::ensure!(
        validate_unit_name(unit_name),
        "invalid remediation unit name"
    );
    let environment = super::super::user_environment::current_user_systemd()?;
    let systemctl = PathBuf::from("/usr/bin/systemctl");
    super::super::super::platform::validate_program_path(&systemctl)?;
    let working =
        crate::harness::execution_security::PrivateDirectory::create("koolade-remediation-stop")?;
    let stop_arguments = ["--user", "stop", unit_name];
    let _ = run_systemctl(
        &systemctl,
        &stop_arguments,
        &environment,
        &working.path,
        Duration::from_secs(5),
    );

    let show_arguments = [
        "--user",
        "show",
        "--property=LoadState",
        "--property=ActiveState",
        unit_name,
    ];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = run_systemctl(
            &systemctl,
            &show_arguments,
            &environment,
            &working.path,
            Duration::from_secs(1),
        )?;
        anyhow::ensure!(
            output.status.success(),
            "remediation service state could not be checked"
        );
        if unit_is_stopped(&String::from_utf8_lossy(&output.stdout)) {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "remediation service scope did not stop"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn run_systemctl(
    systemctl: &std::path::Path,
    arguments: &[&str],
    environment: &super::super::user_environment::UserEnvironment,
    working: &std::path::Path,
    timeout: std::time::Duration,
) -> anyhow::Result<std::process::Output> {
    use std::{
        process::{Command, Stdio},
        thread,
        time::Instant,
    };

    let mut command = Command::new(systemctl);
    command
        .args(arguments)
        .env_clear()
        .env("HOME", &environment.home)
        .env("XDG_RUNTIME_DIR", &environment.runtime_dir)
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .current_dir(working)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(super::super::super::platform::install_linux_process_group_guard);
        }
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => return Ok(child.wait_with_output()?),
            None if Instant::now() < deadline => {
                thread::sleep(std::time::Duration::from_millis(10))
            }
            None => {
                super::super::super::terminate::terminate(pid, &mut child);
                anyhow::bail!("remediation systemd control command timed out");
            }
        }
    }
}

fn unit_is_stopped(properties: &str) -> bool {
    let load_state = properties
        .lines()
        .find_map(|line| line.strip_prefix("LoadState="));
    let active_state = properties
        .lines()
        .find_map(|line| line.strip_prefix("ActiveState="));
    load_state == Some("not-found")
        || (load_state == Some("loaded") && matches!(active_state, Some("inactive" | "failed")))
}

#[cfg(target_os = "linux")]
fn validate_pkexec(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(not(test))]
    use std::os::unix::fs::MetadataExt;

    super::super::super::platform::validate_program_path(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    #[cfg(not(test))]
    anyhow::ensure!(
        metadata.uid() == 0 && metadata.mode() & 0o4000 != 0,
        "pkexec is not the trusted setuid root program"
    );
    #[cfg(test)]
    let _ = metadata;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn validate_pkexec(_path: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("native elevation is unavailable on this platform")
}

#[cfg(test)]
mod tests {
    use super::{unit_is_stopped, validate_unit_name};

    #[test]
    fn elevated_unit_names_are_fixed_prefix_and_uuid_only() {
        assert!(validate_unit_name(
            "koolade-remediation-6ba7b810-9dad-11d1-80b4-00c04fd430c8.service"
        ));
        for invalid in [
            "koolade-remediation-../../docker.service",
            "koolade-remediation-id.service",
            "other-6ba7b810-9dad-11d1-80b4-00c04fd430c8.service",
        ] {
            assert!(!validate_unit_name(invalid), "accepted {invalid}");
        }
    }

    #[test]
    fn stopped_unit_must_be_inactive_failed_or_gone() {
        assert!(unit_is_stopped("LoadState=loaded\nActiveState=inactive\n"));
        assert!(unit_is_stopped("LoadState=loaded\nActiveState=failed\n"));
        assert!(unit_is_stopped(
            "LoadState=not-found\nActiveState=inactive\n"
        ));
        assert!(!unit_is_stopped("LoadState=loaded\nActiveState=active\n"));
        assert!(!unit_is_stopped(
            "LoadState=loaded\nActiveState=deactivating\n"
        ));
        assert!(!unit_is_stopped("ActiveState=inactive\n"));
    }
}
