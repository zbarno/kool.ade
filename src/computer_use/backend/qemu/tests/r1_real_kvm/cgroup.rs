use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

const REQUIRED_CONTROLLERS: [&str; 3] = ["cpu", "memory", "pids"];

/// Verify the kernel cgroup actually contains the exact per-session resource
/// ceilings requested by the signed session, rather than trusting only the
/// backend's write path or QEMU's guest-facing arguments.
pub(super) fn assert_session_limits(
    path: &Path,
    cpu_millis: u32,
    memory_bytes: u64,
    max_processes: u16,
) -> Result<(), String> {
    let expected_cpu_quota = u64::from(cpu_millis).saturating_mul(100);
    assert_control(path, "cpu.max", &format!("{expected_cpu_quota} 100000"))?;
    assert_control(path, "memory.max", &memory_bytes.to_string())?;
    assert_control(path, "memory.swap.max", "0")?;
    assert_control(path, "memory.oom.group", "1")?;
    assert_control(path, "pids.max", &max_processes.to_string())
}

/// Confirm that the live VM has entered its bounded cgroup and remains below
/// its task ceiling. This distinguishes an enforced cgroup from a configured
/// but unused directory.
pub(super) fn assert_live_process_count(path: &Path) -> Result<(), String> {
    let current = read_control(path, "pids.current")?
        .parse::<u64>()
        .map_err(|error| format!("invalid cgroup pids.current: {error}"))?;
    let maximum = read_control(path, "pids.max")?
        .parse::<u64>()
        .map_err(|error| format!("invalid cgroup pids.max: {error}"))?;
    if current == 0 || current > maximum {
        return Err(format!(
            "live cgroup process count {current} is outside 1..={maximum}"
        ));
    }
    Ok(())
}

fn assert_control(path: &Path, name: &str, expected: &str) -> Result<(), String> {
    let actual = read_control(path, name)?;
    if actual != expected {
        return Err(format!(
            "cgroup {name} is {actual:?}, expected {expected:?}"
        ));
    }
    Ok(())
}

fn read_control(path: &Path, name: &str) -> Result<String, String> {
    fs::read_to_string(path.join(name))
        .map(|value| value.trim().to_owned())
        .map_err(|error| format!("read cgroup {name}: {error}"))
}

pub(super) fn current_cgroup_root() -> Result<PathBuf, String> {
    cgroup_root_from(Path::new("/proc/self/cgroup"), Path::new("/sys/fs/cgroup"))
}

fn cgroup_root_from(proc_path: &Path, mount_root: &Path) -> Result<PathBuf, String> {
    let contents = fs::read_to_string(proc_path).map_err(|error| error.to_string())?;
    let mut records = contents.lines().filter_map(|line| {
        let mut fields = line.splitn(3, ':');
        match (fields.next()?, fields.next()?, fields.next()?) {
            ("0", "", path) => Some(path),
            _ => None,
        }
    });
    let relative = records.next().ok_or("unified cgroup path missing")?;
    if records.next().is_some()
        || !relative.starts_with('/')
        || relative
            .split('/')
            .any(|component| matches!(component, "." | ".."))
    {
        return Err("unified cgroup path is invalid".into());
    }
    let current = mount_root.join(relative.trim_start_matches('/'));
    if fs::canonicalize(&current).map_err(|error| error.to_string())? != current {
        return Err("unified cgroup path is not canonical".into());
    }

    let mut found_controllers = false;
    let mut found_process_control = false;
    let mut candidate = current.as_path();
    loop {
        if !candidate.starts_with(mount_root) {
            break;
        }
        if fs::canonicalize(candidate).ok().as_deref() == Some(candidate) {
            let controllers = fs::read_to_string(candidate.join("cgroup.controllers"));
            let enabled = fs::read_to_string(candidate.join("cgroup.subtree_control"));
            if let (Ok(controllers), Ok(enabled)) = (controllers, enabled) {
                let contains = |values: &str, controller: &str| {
                    values
                        .split_ascii_whitespace()
                        .any(|value| value.trim_start_matches('+') == controller)
                };
                if REQUIRED_CONTROLLERS
                    .iter()
                    .all(|name| contains(&controllers, name))
                    && REQUIRED_CONTROLLERS
                        .iter()
                        .all(|name| contains(&enabled, name))
                {
                    found_controllers = true;
                    let process_control = candidate.join("cgroup.procs");
                    if process_control.is_file() {
                        found_process_control = true;
                        if OpenOptions::new()
                            .write(true)
                            .open(&process_control)
                            .is_ok()
                        {
                            return Ok(candidate.to_path_buf());
                        }
                    }
                }
            }
        }
        if candidate == mount_root {
            break;
        }
        let Some(parent) = candidate.parent() else {
            break;
        };
        candidate = parent;
    }
    if !found_controllers {
        return Err("delegated cgroup v2 cpu, memory, and pids controllers are not enabled".into());
    }
    if !found_process_control {
        return Err("delegated cgroup v2 process control is unavailable".into());
    }
    Err("delegated cgroup directory is not writable by the runner account".into())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{assert_live_process_count, assert_session_limits, cgroup_root_from};

    #[test]
    fn live_cgroup_resource_controls_match_the_session_budget() {
        let temporary = tempfile::tempdir().expect("temporary cgroup");
        let path = temporary.path();
        for (name, value) in [
            ("cpu.max", "200000 100000\n"),
            ("memory.max", "2147483648\n"),
            ("memory.swap.max", "0\n"),
            ("memory.oom.group", "1\n"),
            ("pids.max", "128\n"),
            ("pids.current", "73\n"),
        ] {
            fs::write(path.join(name), value).expect("cgroup control");
        }

        assert_session_limits(path, 2_000, 2 * 1024 * 1024 * 1024, 128).unwrap();
        assert_live_process_count(path).unwrap();

        fs::write(path.join("memory.swap.max"), "max\n").unwrap();
        assert!(assert_session_limits(path, 2_000, 2 * 1024 * 1024 * 1024, 128).is_err());
    }

    #[test]
    fn live_cgroup_rejects_empty_or_over_limit_process_counts() {
        let temporary = tempfile::tempdir().expect("temporary cgroup");
        let path = temporary.path();
        fs::write(path.join("pids.max"), "128\n").unwrap();
        fs::write(path.join("pids.current"), "0\n").unwrap();
        assert!(assert_live_process_count(path).is_err());
        fs::write(path.join("pids.current"), "129\n").unwrap();
        assert!(assert_live_process_count(path).is_err());
    }

    #[test]
    fn selects_nearest_writable_delegated_ancestor() {
        let temporary = tempfile::tempdir().expect("temporary cgroup tree");
        let root = temporary.path();
        let delegated = root.join("user.slice/runner.slice");
        fs::create_dir_all(&delegated).expect("delegated cgroup");
        fs::write(delegated.join("cgroup.controllers"), "cpu memory pids\n")
            .expect("available controllers");
        fs::write(
            delegated.join("cgroup.subtree_control"),
            "cpu memory pids\n",
        )
        .expect("enabled controllers");
        fs::write(delegated.join("cgroup.procs"), "").expect("process control");

        let runner = delegated.join("runner.scope");
        fs::create_dir(&runner).expect("runner scope");
        fs::write(runner.join("cgroup.controllers"), "cpu memory pids\n")
            .expect("inherited controllers");
        fs::write(runner.join("cgroup.subtree_control"), "").expect("scope controls");
        fs::write(runner.join("cgroup.procs"), "").expect("scope process control");

        let proc = root.join("proc-cgroup");
        fs::write(&proc, "0::/user.slice/runner.slice/runner.scope\n")
            .expect("process cgroup record");
        assert_eq!(cgroup_root_from(&proc, root).unwrap(), delegated);
    }
}
