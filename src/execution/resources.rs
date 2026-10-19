//! Bounded host resource observations for remote worker scheduling.

use std::{fs, num::NonZeroUsize};

mod cgroup;
use cgroup::{
    cgroup_cpu_pressure_some_avg10_basis_points, cgroup_cpu_quota_millis, cgroup_memory_available,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct HostResourceObservation {
    pub available_cpu_millis: Option<u32>,
    pub available_memory_bytes: Option<u64>,
    pub cpu_pressure_some_avg10_basis_points: Option<u32>,
}

pub(crate) fn observe_host(
    reserved_cpu_millis: Option<u32>,
    reserved_memory_bytes: Option<u64>,
) -> HostResourceObservation {
    // Reserve active Docker execution budgets. Docker enforces these per container;
    // process-cgroup usage alone can miss worker containers in sibling scopes.
    let available_cpu_millis = effective_cpu_capacity_millis().and_then(|capacity| {
        let reserved = reserved_cpu_millis?;
        capacity.checked_sub(reserved)
    });
    let available_memory_bytes = effective_memory_capacity_bytes().and_then(|capacity| {
        let reserved = reserved_memory_bytes?;
        capacity.checked_sub(reserved)
    });
    HostResourceObservation {
        available_cpu_millis,
        available_memory_bytes,
        cpu_pressure_some_avg10_basis_points: observe_cpu_pressure(),
    }
}

fn observe_cpu_pressure() -> Option<u32> {
    let host = fs::read_to_string("/proc/pressure/cpu")
        .ok()
        .and_then(|contents| parse_cpu_pressure_some_avg10_basis_points(&contents));
    let cgroup = fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|groups| {
            cgroup_cpu_pressure_some_avg10_basis_points(&groups, "/sys/fs/cgroup").ok()
        })
        .flatten();
    match (host, cgroup) {
        (Some(host), Some(cgroup)) => Some(host.max(cgroup)),
        (Some(host), None) => Some(host),
        (None, Some(cgroup)) => Some(cgroup),
        (None, None) => None,
    }
}

pub(super) fn parse_cpu_pressure_some_avg10_basis_points(contents: &str) -> Option<u32> {
    let line = contents.lines().find(|line| line.starts_with("some "))?;
    let avg10 = line
        .split_ascii_whitespace()
        .find_map(|field| field.strip_prefix("avg10=")?.parse::<f64>().ok())?;
    if !avg10.is_finite() || !(0.0..=100.0).contains(&avg10) {
        return None;
    }
    u32::try_from((avg10 * 100.0).round() as u64).ok()
}

pub(crate) fn effective_memory_capacity_bytes() -> Option<u64> {
    let cgroups = fs::read_to_string("/proc/self/cgroup").ok()?;
    let host_available = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| parse_available_memory_bytes(&text))?;
    let cgroup_available = cgroup_memory_available(&cgroups, "/sys/fs/cgroup").ok()?;
    Some(cgroup_available.map_or(host_available, |limit| host_available.min(limit)))
}

pub(crate) fn effective_cpu_capacity_millis() -> Option<u32> {
    let cpus = std::thread::available_parallelism().ok()?;
    let cgroups = fs::read_to_string("/proc/self/cgroup").ok()?;
    let quota = cgroup_cpu_quota_millis(&cgroups, "/sys/fs/cgroup").ok()?;
    available_cpu_millis(cpus, quota, 0)
}

#[cfg(test)]
fn cpu_reservation_fits(
    capacity_millis: u32,
    active_millis: u32,
    pending_millis: u32,
    requested_millis: u32,
) -> bool {
    active_millis
        .checked_add(pending_millis)
        .and_then(|total| total.checked_add(requested_millis))
        .is_some_and(|total| total <= capacity_millis)
}

fn available_cpu_millis(
    cpus: NonZeroUsize,
    quota_millis: Option<u32>,
    used_millis: u32,
) -> Option<u32> {
    let total = u32::try_from(cpus.get()).ok()?.checked_mul(1_000)?;
    Some(
        quota_millis
            .map_or(total, |quota| total.min(quota))
            .saturating_sub(used_millis),
    )
}

fn parse_available_memory_bytes(contents: &str) -> Option<u64> {
    let line = contents
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))?;
    let mut fields = line.split_ascii_whitespace();
    fields.next()?;
    let kibibytes = fields.next()?.parse::<u64>().ok()?;
    if fields.next()? != "kB" || fields.next().is_some() {
        return None;
    }
    kibibytes.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::{
        available_cpu_millis, cgroup_cpu_pressure_some_avg10_basis_points, cgroup_cpu_quota_millis,
        cgroup_memory_available, cpu_reservation_fits, parse_available_memory_bytes,
        parse_cpu_pressure_some_avg10_basis_points,
    };
    use std::{collections::HashMap, fs, num::NonZeroUsize};

    #[test]
    fn cpu_availability_subtracts_reserved_docker_budgets() {
        assert_eq!(
            available_cpu_millis(NonZeroUsize::new(4).unwrap(), None, 1_250),
            Some(2_750)
        );
        assert_eq!(
            available_cpu_millis(NonZeroUsize::new(8).unwrap(), Some(1_500), 1_500),
            Some(0)
        );
    }

    #[test]
    fn concurrent_cpu_reservations_do_not_overbook_capacity() {
        let reservations =
            std::sync::Arc::new(std::sync::Mutex::new(HashMap::<String, u32>::new()));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads = (0..2)
            .map(|index| {
                let reservations = reservations.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut reservations = reservations.lock().unwrap();
                    let pending = reservations.values().copied().sum();
                    if cpu_reservation_fits(4_000, 0, pending, 4_000) {
                        reservations.insert(format!("execution-{index}"), 4_000);
                        true
                    } else {
                        false
                    }
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(|accepted| *accepted)
                .count(),
            1
        );
        assert_eq!(reservations.lock().unwrap().len(), 1);
    }

    #[test]
    fn memory_parser_requires_memavailable_and_checked_kibibyte_conversion() {
        assert_eq!(
            parse_available_memory_bytes("MemTotal: 8000 kB\nMemAvailable: 2500 kB\n"),
            Some(2_560_000)
        );
        assert_eq!(parse_available_memory_bytes("MemFree: 2500 kB\n"), None);
        assert_eq!(
            parse_available_memory_bytes("MemAvailable: 18446744073709551615 kB\n"),
            None
        );
    }

    #[test]
    fn cpu_pressure_parser_reads_some_avg10_as_basis_points() {
        assert_eq!(
            parse_cpu_pressure_some_avg10_basis_points(
                "some avg10=2.50 avg60=1.20 avg300=0.50 total=100\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"
            ),
            Some(250)
        );
        assert_eq!(
            parse_cpu_pressure_some_avg10_basis_points("some avg10=100.01 avg60=0.00 total=0\n"),
            None
        );
        assert_eq!(
            parse_cpu_pressure_some_avg10_basis_points("full avg10=1.00\n"),
            None
        );
    }

    #[test]
    fn cgroup_memory_limit_caps_host_availability_and_rejects_untrusted_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("memory.max"), "800000000\n").unwrap();
        fs::write(root.path().join("memory.current"), "300000000\n").unwrap();
        let worker_group = root.path().join("worker.slice");
        fs::create_dir_all(&worker_group).unwrap();
        fs::write(worker_group.join("memory.max"), "1000000000\n").unwrap();
        fs::write(worker_group.join("memory.current"), "400000000\n").unwrap();
        assert_eq!(
            cgroup_memory_available("0::/worker.slice\n", root.path().to_str().unwrap()),
            Ok(Some(500_000_000))
        );
        assert_eq!(
            cgroup_memory_available("0::/../outside\n", root.path().to_str().unwrap()),
            Err(())
        );
    }

    #[test]
    fn cgroup_cpu_quota_caps_available_parallelism() {
        let root = tempfile::tempdir().unwrap();
        let worker_group = root.path().join("worker.slice");
        fs::create_dir_all(&worker_group).unwrap();
        fs::write(worker_group.join("cpu.max"), "150000 100000\n").unwrap();
        assert_eq!(
            cgroup_cpu_quota_millis("0::/worker.slice\n", root.path().to_str().unwrap()),
            Ok(Some(1_500))
        );
        assert_eq!(
            available_cpu_millis(NonZeroUsize::new(8).unwrap(), Some(1_500), 250),
            Some(1_250)
        );
    }

    #[test]
    fn cgroup_cpu_pressure_uses_the_highest_worker_ancestor_observation() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("cpu.pressure"),
            "some avg10=1.25 avg60=1.00 avg300=0.50 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
        )
        .unwrap();
        let worker_group = root.path().join("worker.slice");
        fs::create_dir_all(&worker_group).unwrap();
        fs::write(
            worker_group.join("cpu.pressure"),
            "some avg10=4.00 avg60=2.00 avg300=1.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
        )
        .unwrap();
        assert_eq!(
            cgroup_cpu_pressure_some_avg10_basis_points(
                "0::/worker.slice\n",
                root.path().to_str().unwrap()
            ),
            Ok(Some(400))
        );
        assert_eq!(
            cgroup_cpu_pressure_some_avg10_basis_points(
                "0::/../outside\n",
                root.path().to_str().unwrap()
            ),
            Err(())
        );
    }

    #[test]
    fn cgroup_v1_cpu_and_memory_are_unknown_without_mount_resolution() {
        let root = tempfile::tempdir().unwrap();
        let cpu_group = root.path().join("cpu/worker.slice");
        fs::create_dir_all(&cpu_group).unwrap();
        fs::write(cpu_group.join("cpu.cfs_quota_us"), "50000\n").unwrap();
        fs::write(cpu_group.join("cpu.cfs_period_us"), "100000\n").unwrap();
        assert_eq!(
            cgroup_cpu_quota_millis(
                "2:cpu,cpuacct:/worker.slice\n",
                root.path().to_str().unwrap()
            ),
            Err(())
        );
        assert_eq!(
            cgroup_memory_available("3:memory:/worker.slice\n", root.path().to_str().unwrap()),
            Err(())
        );
    }
}
