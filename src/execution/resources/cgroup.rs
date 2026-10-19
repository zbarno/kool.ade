use std::{fs, path::Path};

pub(super) fn cgroup_memory_available(contents: &str, root: &str) -> Result<Option<u64>, ()> {
    if let Some(path) = contents.lines().find_map(|line| line.strip_prefix("0::")) {
        let directory = cgroup_directory(root, path)?;
        return cgroup_v2_memory_available(&directory, Path::new(root));
    }

    if controller_path(contents, "memory").is_some() {
        // v1 controller mounts and roots vary. Do not guess a memory path.
        return Err(());
    }
    Err(())
}

pub(super) fn cgroup_cpu_quota_millis(contents: &str, root: &str) -> Result<Option<u32>, ()> {
    if let Some(path) = contents.lines().find_map(|line| line.strip_prefix("0::")) {
        let directory = cgroup_directory(root, path)?;
        let quota = cgroup_ancestors(&directory, Path::new(root), |directory| {
            let content = match fs::read_to_string(directory.join("cpu.max")) {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(()),
            };
            let mut fields = content.split_ascii_whitespace();
            let quota = fields.next().ok_or(())?;
            let period = fields.next().ok_or(())?.parse::<u64>().map_err(|_| ())?;
            if fields.next().is_some() || period == 0 {
                return Err(());
            }
            if quota == "max" {
                return Ok(None);
            }
            let quota = quota.parse::<u64>().map_err(|_| ())?;
            Ok(Some(quota.checked_mul(1_000).ok_or(())? / period))
        })?;
        return quota.map(u32::try_from).transpose().map_err(|_| ());
    }

    if controller_path(contents, "cpu").is_some() {
        // v1 controller mount roots are not necessarily `/sys/fs/cgroup/cpu`.
        // Until mountinfo-based resolution is available, unknown is safer than
        // reporting a quota from a guessed path.
        return Err(());
    }
    Ok(None)
}

pub(super) fn cgroup_cpu_pressure_some_avg10_basis_points(
    contents: &str,
    root: &str,
) -> Result<Option<u32>, ()> {
    let path = contents.lines().find_map(|line| line.strip_prefix("0::"));
    let Some(path) = path else {
        return Err(());
    };
    let root = Path::new(root);
    let mut directory = cgroup_directory(root.to_str().ok_or(())?, path)?;
    if !directory.starts_with(root) {
        return Err(());
    }
    let mut pressure = None;
    loop {
        let file = directory.join("cpu.pressure");
        let contents = match fs::read_to_string(file) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(());
            }
            Err(_) => return Err(()),
        };
        let observed = super::parse_cpu_pressure_some_avg10_basis_points(&contents).ok_or(())?;
        pressure = Some(pressure.map_or(observed, |prior: u32| prior.max(observed)));
        if directory == root {
            break;
        }
        directory = directory.parent().ok_or(())?.to_path_buf();
        if !directory.starts_with(root) {
            return Err(());
        }
    }
    Ok(pressure)
}

fn controller_path<'a>(contents: &'a str, controller: &str) -> Option<&'a str> {
    contents.lines().find_map(|line| {
        let (hierarchy, rest) = line.split_once(':')?;
        let (controllers, path) = rest.split_once(':')?;
        (hierarchy != "0" && controllers.split(',').any(|value| value == controller))
            .then_some(path)
    })
}

fn cgroup_directory(root: &str, path: &str) -> Result<std::path::PathBuf, ()> {
    use std::path::Component;
    let relative = Path::new(path).strip_prefix("/").map_err(|_| ())?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(());
    }
    Ok(Path::new(root).join(relative))
}

fn cgroup_v2_memory_available(
    directory: &std::path::Path,
    root: &std::path::Path,
) -> Result<Option<u64>, ()> {
    cgroup_ancestors(directory, root, |directory| {
        let maximum = match fs::read_to_string(directory.join("memory.max")) {
            Ok(maximum) => maximum,
            Err(error) if directory == root && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(_) => return Err(()),
        };
        if maximum.trim() == "max" {
            return Ok(None);
        }
        let maximum = maximum.trim().parse::<u64>().map_err(|_| ())?;
        let current = fs::read_to_string(directory.join("memory.current")).map_err(|_| ())?;
        let current = current.trim().parse::<u64>().map_err(|_| ())?;
        Ok(Some(maximum.saturating_sub(current)))
    })
}

fn cgroup_ancestors(
    directory: &std::path::Path,
    root: &std::path::Path,
    mut read_available: impl FnMut(&std::path::Path) -> Result<Option<u64>, ()>,
) -> Result<Option<u64>, ()> {
    if !directory.starts_with(root) {
        return Err(());
    }
    let mut current = directory;
    let mut available = None;
    loop {
        if let Some(limit) = read_available(current)? {
            available = Some(available.map_or(limit, |prior: u64| prior.min(limit)));
        }
        if current == root {
            break;
        }
        current = current.parent().ok_or(())?;
    }
    Ok(available)
}
