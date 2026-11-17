use std::{fs, path::Path, process::Command};

const CGROUP_ENV: &str = "KOOLADE_R1_KVM_MEMORY_STRESS_CGROUP";
const CHUNK_BYTES: usize = 8 * 1024 * 1024;
const TARGET_PERCENT: u64 = 85;
const MAX_ALLOCATION_BYTES: u64 = 1024 * 1024 * 1024;

/// Apply bounded anonymous-memory pressure from a separate process inside the
/// live VM cgroup. The child stops below memory.max, touches each allocated
/// page, and exits so the parent can verify that the charge is released.
pub(super) fn assert_session_memory_pressure_bounded(
    test_executable: &Path,
    cgroup: &Path,
) -> Result<(), String> {
    let memory_max = read_value(cgroup, "memory.max")?;
    let memory_events_before = read_event(cgroup, "oom_kill")?;
    let baseline_bytes = read_value(cgroup, "memory.current")?;
    let child = Command::new(test_executable)
        .arg("computer_use_r1_memory_pressure_stress_child")
        .arg("--nocapture")
        .env(CGROUP_ENV, cgroup)
        .output()
        .map_err(|error| format!("start bounded cgroup memory-pressure stress: {error}"))?;
    let stdout = String::from_utf8_lossy(&child.stdout);
    if !child.status.success() {
        let stderr = String::from_utf8_lossy(&child.stderr);
        return Err(format!(
            "bounded cgroup memory-pressure stress failed ({}): stdout={stdout}; stderr={stderr}",
            child.status
        ));
    }
    let peak_bytes = read_value(cgroup, "memory.peak")?;
    let current_bytes = read_value(cgroup, "memory.current")?;
    let memory_events_after = read_event(cgroup, "oom_kill")?;
    let target_bytes = memory_max.saturating_mul(TARGET_PERCENT) / 100;
    if peak_bytes < target_bytes {
        return Err(format!(
            "memory-pressure child did not reach its target: peak={peak_bytes} target={target_bytes}"
        ));
    }
    if peak_bytes >= memory_max {
        return Err(format!(
            "memory-pressure child reached the session hard limit: peak={peak_bytes} limit={memory_max}"
        ));
    }
    if current_bytes > baseline_bytes.saturating_add(64 * 1024 * 1024) {
        return Err(format!(
            "memory-pressure child left a charge behind: baseline={baseline_bytes} current={current_bytes}"
        ));
    }
    if memory_events_after != memory_events_before {
        return Err(format!(
            "bounded memory-pressure child unexpectedly triggered an OOM kill: before={memory_events_before} after={memory_events_after}"
        ));
    }
    if !stdout.contains("R1 KVM memory stress: completed=true") {
        return Err(format!(
            "memory-pressure child did not report completion: {stdout}"
        ));
    }
    println!(
        "R1 KVM memory stress: baseline_bytes={baseline_bytes} peak_bytes={peak_bytes} current_bytes={current_bytes} limit_bytes={memory_max} oom_kill_events={memory_events_before}->{memory_events_after}"
    );
    Ok(())
}

/// Test-harness child moved into the session cgroup before allocating. On
/// ordinary test runs it exits without touching host cgroups.
#[test]
fn computer_use_r1_memory_pressure_stress_child() {
    let Some(cgroup) = std::env::var_os(CGROUP_ENV).map(std::path::PathBuf::from) else {
        return;
    };
    let cgroup_procs = cgroup.join("cgroup.procs");
    fs::write(&cgroup_procs, format!("{}\n", std::process::id()))
        .unwrap_or_else(|error| panic!("join the session cgroup: {error}"));
    let members = fs::read_to_string(&cgroup_procs)
        .unwrap_or_else(|error| panic!("verify session cgroup membership: {error}"));
    assert!(
        members
            .lines()
            .any(|pid| pid == std::process::id().to_string()),
        "memory-pressure process is not in the session cgroup"
    );
    let memory_max = read_value(&cgroup, "memory.max")
        .unwrap_or_else(|error| panic!("read session memory.max: {error}"));
    let target_bytes = memory_max.saturating_mul(TARGET_PERCENT) / 100;
    let initial_bytes = read_value(&cgroup, "memory.current")
        .unwrap_or_else(|error| panic!("read initial memory.current: {error}"));
    let required_bytes = target_bytes.saturating_sub(initial_bytes);
    assert!(
        required_bytes > 0 && required_bytes <= MAX_ALLOCATION_BYTES,
        "bounded memory-pressure target is outside the allocation cap: initial={initial_bytes} target={target_bytes} cap={MAX_ALLOCATION_BYTES}"
    );

    let mut allocation = Vec::with_capacity(required_bytes as usize);
    let mut allocated_bytes = 0_u64;
    while allocated_bytes < required_bytes {
        let remaining = required_bytes - allocated_bytes;
        let chunk_size = remaining.min(CHUNK_BYTES as u64) as usize;
        let previous_len = allocation.len();
        allocation.resize(previous_len + chunk_size, 1);
        // Touch each page so the cgroup observes resident anonymous memory.
        for page in allocation[previous_len..].iter().step_by(4096) {
            std::hint::black_box(*page);
        }
        allocated_bytes += chunk_size as u64;
        let current_bytes = read_value(&cgroup, "memory.current")
            .unwrap_or_else(|error| panic!("sample memory.current: {error}"));
        if current_bytes >= target_bytes {
            break;
        }
        assert!(
            current_bytes < memory_max,
            "memory-pressure child reached memory.max before its target: current={current_bytes} target={target_bytes} max={memory_max}"
        );
    }
    let peak_bytes = read_value(&cgroup, "memory.peak")
        .unwrap_or_else(|error| panic!("read session memory.peak: {error}"));
    assert!(
        peak_bytes >= target_bytes,
        "memory-pressure child missed the target: peak={peak_bytes} target={target_bytes}"
    );
    assert!(
        peak_bytes < memory_max,
        "memory-pressure child reached the hard limit: peak={peak_bytes} max={memory_max}"
    );
    drop(allocation);
    println!(
        "R1 KVM memory stress: completed=true allocated_bytes={allocated_bytes} peak_bytes={peak_bytes} limit_bytes={memory_max}"
    );
}

fn read_value(directory: &Path, name: &str) -> Result<u64, String> {
    fs::read_to_string(directory.join(name))
        .map_err(|error| format!("read session cgroup {name}: {error}"))?
        .trim()
        .parse()
        .map_err(|error| format!("parse session cgroup {name}: {error}"))
}

fn read_event(directory: &Path, name: &str) -> Result<u64, String> {
    let contents = fs::read_to_string(directory.join("memory.events"))
        .map_err(|error| format!("read session cgroup memory.events: {error}"))?;
    contents
        .lines()
        .find_map(|line| {
            let (event, value) = line.split_once(' ')?;
            (event == name).then_some(value)
        })
        .ok_or_else(|| format!("session cgroup memory.events has no {name} event"))?
        .parse()
        .map_err(|error| format!("parse session cgroup memory.events {name}: {error}"))
}
