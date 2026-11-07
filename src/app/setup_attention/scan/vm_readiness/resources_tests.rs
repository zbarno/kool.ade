use super::*;
use crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1 as budget;

#[test]
fn low_memory_or_storage_blocks_storage_readiness() {
    let enough = ResourceCapacity {
        available_memory_bytes: Some(budget.memory_bytes),
        available_storage_bytes: Some(budget.disk_bytes),
        filesystem_family: Some(
            crate::app::setup_attention::catalog::LinuxVmStorageFilesystemV1::Ext,
        ),
    };
    assert_eq!(enough.state(), EvidenceState::Ready);
    assert_eq!(enough.code(), EvidenceCode::LinuxVmResourcesAvailable);

    let low_memory = ResourceCapacity {
        available_memory_bytes: Some(budget.memory_bytes - 1),
        ..enough
    };
    assert_eq!(low_memory.state(), EvidenceState::Missing);

    let low_storage = ResourceCapacity {
        available_storage_bytes: Some(budget.disk_bytes - 1),
        ..enough
    };
    assert_eq!(low_storage.state(), EvidenceState::Missing);
    assert_eq!(low_storage.shortages(), (false, true));
}

#[test]
fn storage_probe_records_only_a_bounded_filesystem_family() {
    use crate::app::setup_attention::catalog::LinuxVmStorageFilesystemV1 as Filesystem;
    assert_eq!(storage_filesystem_family(0xef53), Filesystem::Ext);
    assert_eq!(storage_filesystem_family(0x5846_5342), Filesystem::Xfs);
    assert_eq!(storage_filesystem_family(0x9123_683e), Filesystem::Btrfs);
    assert_eq!(storage_filesystem_family(0x794c_7630), Filesystem::Other);
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: the storage probe observes the nearest existing parent
// using normal kernel symlink resolution without creating the planned path.
// Uses only temporary directories and host statvfs/statfs syscalls.
fn storage_probe_is_read_only_and_uses_normal_symlink_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let planned = temp.path().join("not-created").join("overlay.qcow2");
    let capacity = storage_capacity_on_parent(&planned).unwrap();
    assert!(capacity.available_bytes > 0);
    assert!(!planned.parent().unwrap().exists());

    let link = temp.path().join("linked-storage");
    std::os::unix::fs::symlink(temp.path(), &link).unwrap();
    let through_link = storage_capacity_on_parent(&link.join("overlay.qcow2")).unwrap();
    assert!(through_link.available_bytes > 0);
    assert!(!link.join("overlay.qcow2").exists());
}

#[test]
fn missing_measurements_are_unknown_unless_another_measurement_is_low() {
    let missing = ResourceCapacity::default();
    assert_eq!(missing.state(), EvidenceState::Unknown);
    assert_eq!(missing.code(), EvidenceCode::LinuxVmResourcesUnknown);

    let known_low = ResourceCapacity {
        available_storage_bytes: Some(budget.disk_bytes - 1),
        ..missing
    };
    assert_eq!(known_low.state(), EvidenceState::Missing);
}
