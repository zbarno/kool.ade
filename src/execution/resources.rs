//! Bounded host resource observations for remote worker scheduling.

use std::{fs, io::Read, num::NonZeroUsize};

const MAX_RESOURCE_PROC_BYTES: usize = 64 * 1024;

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
    let cgroups = read_bounded_proc_file("/proc/self/cgroup")?;
    let host_available = read_bounded_proc_file("/proc/meminfo")
        .and_then(|text| parse_available_memory_bytes(&text))?;
    let cgroup_available = cgroup_memory_available(&cgroups, "/sys/fs/cgroup").ok()?;
    Some(cap_available_memory_bytes(host_available, cgroup_available))
}

fn cap_available_memory_bytes(host_available: u64, cgroup_available: Option<u64>) -> u64 {
    cgroup_available.map_or(host_available, |limit| host_available.min(limit))
}

fn read_bounded_proc_file(path: &str) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_RESOURCE_PROC_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_RESOURCE_PROC_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn effective_cpu_capacity_millis() -> Option<u32> {
    let cpus = std::thread::available_parallelism().ok()?;
    let cgroups = fs::read_to_string("/proc/self/cgroup").ok()?;
    let quota = cgroup_cpu_quota_millis(&cgroups, "/sys/fs/cgroup").ok()?;
    available_cpu_millis(cpus, quota, 0)
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
#[path = "resources/tests.rs"]
mod tests;
