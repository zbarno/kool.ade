use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

const REQUIRED_CONTROLLERS: [&str; 3] = ["cpu", "memory", "pids"];

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

    use super::cgroup_root_from;

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
