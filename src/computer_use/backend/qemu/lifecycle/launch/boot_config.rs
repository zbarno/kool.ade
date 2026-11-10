use crate::computer_use::{ComputerSessionV1, guest_protocol::GuestBootConfigV1};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use zeroize::Zeroizing;

const MAX_APP_CATALOG_BYTES: usize = 256 * 1024;

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

pub(super) fn write_application_catalog(
    bytes: &[u8],
    path: &Path,
) -> Result<(), crate::computer_use::ComputerBackendErrorV1> {
    if bytes.is_empty() || bytes.len() > MAX_APP_CATALOG_BYTES {
        return Err(crate::computer_use::ComputerBackendErrorV1::ResourceLimit);
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.file_type().is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0)
    {
        return Err(crate::computer_use::ComputerBackendErrorV1::OwnershipMismatch);
    }
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(path)
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    file.write_all(bytes)
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    file.sync_all()
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)
}

pub(super) fn write_app_helper_config(
    session: &ComputerSessionV1,
    controller_key_id: &str,
    controller_key: &ed25519_dalek::VerifyingKey,
    path: &Path,
) -> Result<(), crate::computer_use::ComputerBackendErrorV1> {
    if session.controller_proof.signer_key_id != controller_key_id {
        return Err(crate::computer_use::ComputerBackendErrorV1::InvalidSession);
    }
    let config = crate::computer_use::apps::guest_helper::GuestAppHelperConfigV1 {
        schema_version: 1,
        session_id: session.identity.session_id.clone(),
        worker_id: match &session.target {
            crate::computer_use::ComputerTargetV1::LocalKvm { machine_id } => machine_id.clone(),
        },
        image_digest: session.image_digest.clone(),
        session_class: session.session_class,
        lease_generation: session.lease.generation,
        approval_generation: session.approval_generation,
        cancellation_generation: session.cancellation_generation,
        initial_action_sequence: session.action_sequence,
        expires_at_unix_ms: session.lease.expires_at_unix_ms,
        boot_id: uuid::Uuid::new_v4().to_string(),
        allowed_apps: session.allowed_apps.iter().cloned().collect(),
        controller_key_id: controller_key_id.to_owned(),
        controller_verifying_key_hex: controller_key
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    };
    config
        .validate()
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::InvalidSession)?;
    let bytes =
        serde_json::to_vec(&config).map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    if bytes.len() > 4096 {
        return Err(crate::computer_use::ComputerBackendErrorV1::ResourceLimit);
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.file_type().is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0)
    {
        return Err(crate::computer_use::ComputerBackendErrorV1::OwnershipMismatch);
    }
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(path)
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    file.write_all(&bytes)
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)?;
    file.sync_all()
        .map_err(|_| crate::computer_use::ComputerBackendErrorV1::Io)
}
