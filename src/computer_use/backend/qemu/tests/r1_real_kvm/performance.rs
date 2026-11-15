use std::{fs, path::Path};

pub(super) fn read_cgroup_value(cgroup_directory: &Path, name: &str) -> Result<u64, String> {
    let path = cgroup_directory.join(name);
    fs::read_to_string(&path)
        .map_err(|error| format!("read cgroup metric {}: {error}", path.display()))?
        .trim()
        .parse()
        .map_err(|error| format!("parse cgroup metric {}: {error}", path.display()))
}

pub(super) fn read_cpu_usage_usec(cgroup_directory: &Path) -> Result<u64, String> {
    let path = cgroup_directory.join("cpu.stat");
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("read cgroup metric {}: {error}", path.display()))?;
    contents
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(' ')?;
            (name == "usage_usec").then_some(value)
        })
        .ok_or_else(|| format!("cgroup CPU metric is unavailable: {}", path.display()))?
        .parse()
        .map_err(|error| format!("parse cgroup CPU metric {}: {error}", path.display()))
}
