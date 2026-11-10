//! Persisted Computer Use worker enrollment, separate from Docker workers.

use crate::computer_use::remote_worker::ComputerRemoteWorkerEnrollmentV1;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const DIRECTORY: &str = "computer-use-remote-workers/v1";
const MAX_ENROLLMENT_BYTES: u64 = 32 * 1024;

/// Store an operator-provisioned worker binding. Re-enrollment cannot silently
/// replace a worker's machine, SSH key, or attestation key.
pub fn enroll(enrollment: ComputerRemoteWorkerEnrollmentV1) -> anyhow::Result<()> {
    enrollment.validate().map_err(anyhow::Error::msg)?;
    let root = crate::persistence::state_root();
    let path = enrollment_path(&root, &enrollment.worker_id)?;
    create_private_directory(path.parent().expect("enrollment path parent"))?;
    let bytes = serde_json::to_vec(&enrollment)?;
    match crate::artifacts::atomic_create_bytes(&path, &bytes) {
        Ok(()) => {
            set_private_file(&path)?;
            Ok(())
        }
        Err(_error) if path.exists() => {
            let existing = get(&enrollment.worker_id)?
                .ok_or_else(|| anyhow::anyhow!("existing worker enrollment disappeared"))?;
            anyhow::ensure!(
                same_binding(&existing, &enrollment),
                "worker ID is already bound to a different Computer Use identity"
            );
            anyhow::ensure!(
                existing == enrollment,
                "worker enrollment already exists; explicit update is required"
            );
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub fn get(worker_id: &str) -> anyhow::Result<Option<ComputerRemoteWorkerEnrollmentV1>> {
    let path = enrollment_path(&crate::persistence::state_root(), worker_id)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Computer Use enrollment is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
            "Computer Use enrollment ownership or permissions are unsafe"
        );
    }
    anyhow::ensure!(
        metadata.len() <= MAX_ENROLLMENT_BYTES,
        "Computer Use enrollment exceeds size limit"
    );
    let file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_ENROLLMENT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_ENROLLMENT_BYTES,
        "Computer Use enrollment exceeds size limit"
    );
    let enrollment: ComputerRemoteWorkerEnrollmentV1 = serde_json::from_slice(&bytes)?;
    enrollment.validate().map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        enrollment.worker_id == worker_id,
        "Computer Use enrollment identity mismatch"
    );
    Ok(Some(enrollment))
}

pub fn list() -> anyhow::Result<Vec<ComputerRemoteWorkerEnrollmentV1>> {
    let directory = crate::persistence::state_root().join(DIRECTORY);
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut enrollments = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(worker_id) = name.strip_suffix(".json") else {
            continue;
        };
        if let Some(enrollment) = get(worker_id)? {
            enrollments.push(enrollment);
        }
    }
    enrollments.sort_by(|left, right| left.worker_id.cmp(&right.worker_id));
    Ok(enrollments)
}

/// Persist the operator's selected worker. This is only a selection record;
/// it cannot enroll or rebind a worker and does not open the release gate.
pub fn select(worker_id: &str) -> anyhow::Result<ComputerRemoteWorkerEnrollmentV1> {
    let enrollment =
        get(worker_id)?.ok_or_else(|| anyhow::anyhow!("Computer Use worker is not enrolled"))?;
    anyhow::ensure!(enrollment.enabled, "Computer Use worker is disabled");
    let root = crate::persistence::state_root();
    let path = root.join(DIRECTORY).join("selected.json");
    create_private_directory(path.parent().expect("selection path parent"))?;
    let bytes = serde_json::to_vec(&SelectedWorkerV1 {
        schema_version: 1,
        worker_id: worker_id.to_owned(),
    })?;
    crate::artifacts::atomic_write_bytes(&path, &bytes)?;
    set_private_file(&path)?;
    Ok(enrollment)
}

pub fn selected() -> anyhow::Result<Option<ComputerRemoteWorkerEnrollmentV1>> {
    let root = crate::persistence::state_root();
    let path = root.join(DIRECTORY).join("selected.json");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 1024,
        "Computer Use selection file is unsafe"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
            "Computer Use selection ownership or permissions are unsafe"
        );
    }
    let value: SelectedWorkerV1 = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        value.schema_version == 1,
        "Computer Use selection schema is unsupported"
    );
    let enrollment = get(&value.worker_id)?
        .ok_or_else(|| anyhow::anyhow!("selected Computer Use worker is no longer enrolled"))?;
    anyhow::ensure!(
        enrollment.enabled,
        "selected Computer Use worker is disabled"
    );
    Ok(Some(enrollment))
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SelectedWorkerV1 {
    schema_version: u16,
    worker_id: String,
}

fn same_binding(
    left: &ComputerRemoteWorkerEnrollmentV1,
    right: &ComputerRemoteWorkerEnrollmentV1,
) -> bool {
    left.worker_id == right.worker_id
        && left.machine_id == right.machine_id
        && left.ssh_host_alias == right.ssh_host_alias
        && left.ssh_principal == right.ssh_principal
        && left.ssh_host_key_fingerprint == right.ssh_host_key_fingerprint
        && left.worker_attestation_key_id == right.worker_attestation_key_id
        && left.worker_attestation_verifying_key_hex == right.worker_attestation_verifying_key_hex
}

fn enrollment_path(root: &Path, worker_id: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !worker_id.is_empty()
            && worker_id.len() <= 128
            && worker_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')),
        "worker ID is invalid"
    );
    Ok(root.join(DIRECTORY).join(format!("{worker_id}.json")))
}

fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Computer Use enrollment directory is not trusted"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_private_file(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
