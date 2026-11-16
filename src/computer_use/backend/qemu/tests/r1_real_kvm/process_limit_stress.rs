use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const CGROUP_ENV: &str = "KOOLADE_R1_KVM_PROCESS_STRESS_CGROUP";

/// Exercise the kernel's live task limit without creating unbounded work. The
/// isolated child moves only itself into the VM cgroup, creates blocked threads
/// until `pids.max` rejects another, then joins every thread before exiting.
pub(super) fn assert_session_task_limit_enforced(
    test_executable: &Path,
    cgroup: &Path,
) -> Result<(), String> {
    let before = read_event(cgroup, "max")?;
    let baseline_tasks = read_value(cgroup, "pids.current")?;
    let child = Command::new(test_executable)
        .arg("computer_use_r1_process_limit_stress_child")
        .arg("--nocapture")
        .env(CGROUP_ENV, cgroup)
        .output()
        .map_err(|error| format!("start bounded cgroup process-limit stress: {error}"))?;
    if !child.status.success() {
        let stderr = String::from_utf8_lossy(&child.stderr);
        let stdout = String::from_utf8_lossy(&child.stdout);
        return Err(format!(
            "bounded cgroup process-limit stress failed ({}): stdout={stdout}; stderr={stderr}",
            child.status
        ));
    }
    let after = read_event(cgroup, "max")?;
    if after <= before {
        return Err(format!(
            "kernel pids.max did not record a denied task creation: before={before}, after={after}"
        ));
    }
    let remaining_tasks = read_value(cgroup, "pids.current")?;
    if remaining_tasks > baseline_tasks.saturating_add(4) {
        return Err(format!(
            "process-limit stress left tasks in the VM cgroup: baseline={baseline_tasks}, current={remaining_tasks}"
        ));
    }
    let output = String::from_utf8_lossy(&child.stdout);
    if !output.contains("R1 KVM pids stress: denied_extra_task=true") {
        return Err(format!(
            "process-limit child did not report a bounded task denial: {output}"
        ));
    }
    println!(
        "R1 KVM pids stress: baseline_tasks={baseline_tasks} current_tasks={remaining_tasks} max_events={before}->{after}"
    );
    Ok(())
}

/// This test is invoked in a separate test process by the real-KVM parent. On
/// ordinary test runs it exits without touching host cgroups.
#[test]
fn computer_use_r1_process_limit_stress_child() {
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
        "stress process is not in the session cgroup"
    );

    let maximum = read_value(&cgroup, "pids.max")
        .unwrap_or_else(|error| panic!("read session pids.max: {error}"));
    let stop = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();
    let mut denied_extra_task = false;
    for _ in 0..maximum.saturating_add(16) {
        let stop_worker = Arc::clone(&stop);
        match thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || {
                while !stop_worker.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(10));
                }
            }) {
            Ok(worker) => workers.push(worker),
            Err(_) => {
                denied_extra_task = true;
                break;
            }
        }
    }
    let workers_created = workers.len();
    stop.store(true, Ordering::Release);
    for worker in workers {
        worker
            .join()
            .unwrap_or_else(|_| panic!("bounded cgroup stress worker panicked"));
    }
    assert!(
        denied_extra_task,
        "session pids.max did not reject an additional bounded worker"
    );
    println!(
        "R1 KVM pids stress: denied_extra_task=true workers_created={}",
        workers_created
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
    let contents = fs::read_to_string(directory.join("pids.events"))
        .map_err(|error| format!("read session cgroup pids.events: {error}"))?;
    contents
        .lines()
        .find_map(|line| {
            let (event, count) = line.split_once(' ')?;
            (event == name).then_some(count)
        })
        .ok_or_else(|| format!("session cgroup pids.events has no {name} event"))?
        .parse()
        .map_err(|error| format!("parse session cgroup pids.events {name}: {error}"))
}
