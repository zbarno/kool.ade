use crate::computer_use::{ComputerSessionV1, guest_protocol::GuestBootConfigV1};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use zeroize::Zeroizing;

pub(super) fn write_boot_config(
    session: &ComputerSessionV1,
    path: &Path,
) -> Result<(), crate::computer_use::ComputerBackendErrorV1> {
    let worker_id = match &session.target {
        crate::computer_use::ComputerTargetV1::LocalKvm { machine_id } => machine_id.clone(),
    };
    let mut allowed_apps = session.allowed_apps.iter().cloned().collect::<Vec<_>>();
    allowed_apps.sort();
    let config = GuestBootConfigV1::issue(
        session.identity.session_id.clone(),
        worker_id,
        session.lease.generation,
        session.action_sequence,
        session.lease.expires_at_unix_ms,
        allowed_apps,
    )
    .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    let bytes = Zeroizing::new(
        serde_json::to_vec(&config).map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?,
    );
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.file_type().is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0)
    {
        return Err(crate::computer_use::ComputerBackendErrorV1::OwnershipMismatch);
    }
    let mut output = OpenOptions::new();
    output.write(true).create(true).truncate(true);
    output
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = output
        .open(path)
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    file.write_all(&bytes)
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    file.sync_all()
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)
}
