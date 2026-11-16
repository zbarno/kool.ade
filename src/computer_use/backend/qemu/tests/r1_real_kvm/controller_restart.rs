use std::{
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::computer_use::{ComputerBackend, ComputerBackendEventKindV1, ComputerResourceLimitsV1};

use super::{
    CONTROLLER_RESTART_CHILD_ENV, CONTROLLER_RESTART_ROOT_ENV, LocalKvmComputerBackend,
    cgroup::current_cgroup_root, cleanup, target_machine_id,
};

const READY_FILE: &str = "controller-restart-ready.json";
const CONTROLLER_KEY_ID: &str = "00000000-0000-4000-8000-000000000008";

#[derive(serde::Deserialize, serde::Serialize)]
struct ReadyRecord {
    session_id: String,
    lease_generation: u64,
    machine_id: String,
}

/// Production boundary: a controller process owning a live QEMU/KVM session
/// exits abruptly, then a new process reconstructs the backend and recovers
/// its durable owner record. In-process backend recreation cannot prove the
/// OS process boundary or orphan-QEMU discovery. Requires the same trusted
/// real-KVM host as the parent acceptance test.
pub(super) fn run() -> Result<(), String> {
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = temporary.path();
    let ready_path = root.join(READY_FILE);
    let log_path = root.join("controller-process.log");
    let log = fs::File::create(&log_path).map_err(|error| error.to_string())?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;
    let test_executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut child = Command::new(test_executable)
        .arg("computer_use_r1_real_kvm_acceptance")
        .arg("--nocapture")
        .env("KOOLADE_COMPUTER_USE_R1_REAL_KVM", "1")
        .env(CONTROLLER_RESTART_CHILD_ENV, "1")
        .env(CONTROLLER_RESTART_ROOT_ENV, root)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("start isolated controller process: {error}"))?;

    let wait_result = wait_until_ready(&mut child, &ready_path, &log_path);
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        child
            .kill()
            .map_err(|error| format!("force-stop old controller process: {error}"))?;
    }
    let controller_status = child
        .wait()
        .map_err(|error| format!("reap old controller process: {error}"))?;

    let qemu_state_before_recovery = wait_result
        .as_ref()
        .ok()
        .map(|ready| qemu_process_state(root, &ready.session_id));
    let fallback_machine_id = target_machine_id(&super::super::session(), None)
        .map_err(|error| format!("recovery target: {error:?}"))?
        .to_owned();
    let recovery_machine_id = wait_result
        .as_ref()
        .map(|ready| ready.machine_id.as_str())
        .unwrap_or(&fallback_machine_id);
    let recovery_result = if root.join("runtime/sessions").exists() {
        let recovery_started = Instant::now();
        recover_from_fresh_controller(root, recovery_machine_id)
            .map(|events| (events, recovery_started.elapsed().as_millis()))
    } else {
        Ok((Vec::new(), 0))
    };
    let ready = match wait_result {
        Ok(ready) => ready,
        Err(error) => {
            return match recovery_result {
                Ok(_) => Err(error),
                Err(cleanup_error) => Err(format!(
                    "{error}; recovery cleanup also failed: {cleanup_error}"
                )),
            };
        }
    };
    let (recovered, recovery_ms) = recovery_result?;
    let (qemu_pid, qemu_was_alive) =
        qemu_state_before_recovery.ok_or("controller-ready record is missing")??;
    if !qemu_was_alive {
        return Err("QEMU did not remain alive after the old controller was killed".into());
    }
    if controller_status.signal() != Some(libc::SIGKILL) {
        return Err(format!(
            "old controller did not exit from the injected SIGKILL: {controller_status}"
        ));
    }
    if recovered.len() != 1
        || recovered[0].session_id != ready.session_id
        || recovered[0].lease_generation != ready.lease_generation
        || recovered[0].kind != ComputerBackendEventKindV1::Recovered
    {
        return Err(format!(
            "unexpected full-process-restart recovery: {recovered:?}"
        ));
    }
    if process_matches_session(qemu_pid, &ready.session_id)? {
        return Err("recovery left the session's QEMU process alive".into());
    }

    let runtime = root.join("runtime/sessions").join(&ready.session_id);
    let cgroup_root = current_cgroup_root()?;
    let session_cgroup = cgroup_root.join(format!("koolade-{}", ready.session_id));
    cleanup::assert_released(&runtime, &session_cgroup, &root.join("overlays"))?;
    println!(
        "R1 real KVM controller restart evidence: controller_signal={} recovered_events={} recovery_ms={} session={}",
        controller_status.signal().unwrap_or_default(),
        recovered.len(),
        recovery_ms,
        ready.session_id,
    );
    Ok(())
}

fn qemu_process_state(root: &Path, session_id: &str) -> Result<(u32, bool), String> {
    let owner_path = root
        .join("runtime/sessions")
        .join(session_id)
        .join("owner.json");
    let owner: serde_json::Value = serde_json::from_slice(
        &fs::read(owner_path).map_err(|error| format!("read durable QEMU owner: {error}"))?,
    )
    .map_err(|error| format!("parse durable QEMU owner: {error}"))?;
    let pid = owner
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .ok_or("durable QEMU owner has no pid")?;
    let pid = u32::try_from(pid).map_err(|error| format!("invalid durable QEMU pid: {error}"))?;
    Ok((pid, process_matches_session(pid, session_id)?))
}

fn process_matches_session(pid: u32, session_id: &str) -> Result<bool, String> {
    match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(command_line) => Ok(command_line
            .windows(session_id.len())
            .any(|part| part == session_id.as_bytes())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("read owned QEMU command line: {error}")),
    }
}

pub(super) fn publish_ready(
    root: &Path,
    session_id: &str,
    lease_generation: u64,
    machine_id: &str,
) -> Result<(), String> {
    let record = ReadyRecord {
        session_id: session_id.to_owned(),
        lease_generation,
        machine_id: machine_id.to_owned(),
    };
    let temporary = root.join(format!("{READY_FILE}.tmp"));
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    let mut file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::rename(temporary, root.join(READY_FILE)).map_err(|error| error.to_string())
}

fn wait_until_ready(
    child: &mut std::process::Child,
    ready_path: &Path,
    log_path: &Path,
) -> Result<ReadyRecord, String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if ready_path.exists() {
            let bytes = fs::read(ready_path).map_err(|error| error.to_string())?;
            return serde_json::from_slice(&bytes)
                .map_err(|error| format!("parse controller-ready record: {error}"));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("inspect controller process: {error}"))?
        {
            let log = fs::read_to_string(log_path).unwrap_or_default();
            return Err(format!(
                "controller process exited before it owned a ready VM ({status}): {log}"
            ));
        }
        if Instant::now() >= deadline {
            let log = fs::read_to_string(log_path).unwrap_or_default();
            return Err(format!(
                "controller did not reach a live KVM session within five minutes: {log}"
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn recover_from_fresh_controller(
    root: &Path,
    machine_id: &str,
) -> Result<Vec<crate::computer_use::ComputerBackendEventV1>, String> {
    let session = super::super::session();
    let expected_machine_id = target_machine_id(&session, None)
        .map_err(|error| format!("recovery target: {error:?}"))?
        .to_owned();
    if expected_machine_id != machine_id {
        return Err("controller-ready record names a different host machine".into());
    }
    let recovery_backend = LocalKvmComputerBackend::new(
        root.join("runtime"),
        root.join("overlays"),
        current_cgroup_root()?,
        machine_id.to_owned(),
        CONTROLLER_KEY_ID.to_owned(),
        ed25519_dalek::SigningKey::from_bytes(&[7; 32]).verifying_key(),
        ComputerResourceLimitsV1::default(),
    )
    .map_err(|error| format!("new controller backend: {error:?}"))?;
    recovery_backend
        .recover()
        .map_err(|error| format!("recover after controller restart: {error:?}"))
}
