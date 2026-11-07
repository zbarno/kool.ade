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

use super::{
    MAX_RESOURCE_PROC_BYTES, available_cpu_millis, cap_available_memory_bytes,
    cgroup_cpu_pressure_some_avg10_basis_points, cgroup_cpu_quota_millis, cgroup_memory_available,
    parse_available_memory_bytes, parse_cpu_pressure_some_avg10_basis_points,
    read_bounded_proc_file,
};
use std::{collections::HashMap, fs, num::NonZeroUsize};

#[test]
fn effective_memory_uses_the_lower_host_or_cgroup_headroom() {
    assert_eq!(
        cap_available_memory_bytes(4_000_000_000, Some(500_000_000)),
        500_000_000
    );
    assert_eq!(
        cap_available_memory_bytes(300_000_000, Some(500_000_000)),
        300_000_000
    );
    assert_eq!(
        cap_available_memory_bytes(4_000_000_000, None),
        4_000_000_000
    );
}

#[test]
fn proc_resource_inputs_are_bounded_and_require_utf8() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("resource-probe");
    fs::write(&path, vec![b'x'; MAX_RESOURCE_PROC_BYTES + 1]).unwrap();
    assert_eq!(read_bounded_proc_file(path.to_str().unwrap()), None);

    fs::write(&path, "0::/worker.slice\n").unwrap();
    assert_eq!(
        read_bounded_proc_file(path.to_str().unwrap()).as_deref(),
        Some("0::/worker.slice\n")
    );

    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert_eq!(read_bounded_proc_file(path.to_str().unwrap()), None);
}

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
    let reservations = std::sync::Arc::new(std::sync::Mutex::new(HashMap::<String, u32>::new()));
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
