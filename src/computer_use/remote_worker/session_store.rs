//! Private controller-side recovery intent for remote Computer Use sessions.

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::OpenOptions,
    path::{Path, PathBuf},
};

const MAX_RECORD_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerRecoveryRecordV1 {
    pub schema_version: u16,
    pub worker_id: String,
    pub machine_id: String,
    pub session: crate::computer_use::ComputerSessionV1,
    /// Highest stream sequence durably reserved before sending an RPC.
    pub last_transport_sequence: u64,
    pub last_event_sequence: u64,
    /// An action reserved before a potentially ambiguous write. It is never replayed.
    pub pending_action_sequence: Option<u64>,
    pub outcome_unknown: bool,
}

impl ComputerRemoteWorkerRecoveryRecordV1 {
    pub fn validate(&self, worker_id: &str, machine_id: &str) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.worker_id != worker_id
            || self.machine_id != machine_id
            || self.session.target.worker_id() != worker_id
            || self.session.target.target_machine_id() != machine_id
            || self.session.target.validate().is_err()
            || self.session.identity.validate().is_err()
            || (self.last_transport_sequence == 0 && !self.outcome_unknown)
            || self.last_event_sequence > 10_000_000
            || self
                .pending_action_sequence
                .is_some_and(|sequence| sequence == 0)
            || (self.pending_action_sequence.is_some() && !self.outcome_unknown)
        {
            return Err("remote Computer Use recovery record is invalid");
        }
        Ok(())
    }
}

/// Durable journal for one remote controller. Files contain no private keys,
/// SSH credentials, host paths, or guest artifact bytes.
#[derive(Clone)]
pub struct ComputerRemoteWorkerSessionStoreV1 {
    root: PathBuf,
}

impl ComputerRemoteWorkerSessionStoreV1 {
    pub fn open(root: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&root)?;
        secure_directory(&root)?;
        Ok(Self { root })
    }

    pub fn save(&self, record: &ComputerRemoteWorkerRecoveryRecordV1) -> std::io::Result<()> {
        let _lock = self.lock_exclusive()?;
        validate_record_shape(record)?;
        let path = self.path(&record.session.identity.session_id)?;
        let bytes = serde_json::to_vec(record)
            .map_err(|_| invalid_data("recovery record cannot be serialized"))?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(invalid_data("recovery record exceeds size limit"));
        }
        crate::artifacts::atomic_write_bytes(&path, &bytes)
            .map_err(|_| invalid_data("recovery record could not be persisted"))?;
        secure_file(&path)?;
        Ok(())
    }

    pub fn load(
        &self,
        session_id: &str,
    ) -> std::io::Result<Option<ComputerRemoteWorkerRecoveryRecordV1>> {
        let _lock = self.lock_shared()?;
        let path = self.path(session_id)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        validate_file(&metadata)?;
        if metadata.len() > MAX_RECORD_BYTES {
            return Err(invalid_data("recovery record exceeds size limit"));
        }
        let record: ComputerRemoteWorkerRecoveryRecordV1 = serde_json::from_slice(&fs::read(path)?)
            .map_err(|_| invalid_data("recovery record is malformed"))?;
        validate_record_shape(&record)?;
        if record.session.identity.session_id != session_id {
            return Err(invalid_data("recovery record session identity mismatch"));
        }
        Ok(Some(record))
    }

    pub fn list(&self) -> std::io::Result<Vec<ComputerRemoteWorkerRecoveryRecordV1>> {
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(session_id) = name.strip_suffix(".json") else {
                continue;
            };
            if let Some(record) = self.load(session_id)? {
                records.push(record);
            }
        }
        records.sort_by(|a, b| {
            a.session
                .identity
                .session_id
                .cmp(&b.session.identity.session_id)
        });
        Ok(records)
    }

    pub fn remove(&self, session_id: &str) -> std::io::Result<()> {
        let _lock = self.lock_exclusive()?;
        let path = self.path(session_id)?;
        match fs::remove_file(&path) {
            Ok(()) => crate::artifacts::sync_parent_directory(&path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn path(&self, session_id: &str) -> std::io::Result<PathBuf> {
        if !uuid::Uuid::parse_str(session_id)
            .is_ok_and(|value| value.hyphenated().to_string() == session_id)
        {
            return Err(invalid_data("recovery session ID is invalid"));
        }
        Ok(self.root.join(format!("{session_id}.json")))
    }

    fn lock_shared(&self) -> std::io::Result<fs::File> {
        let lock = self.open_lock()?;
        FileExt::lock_shared(&lock)?;
        Ok(lock)
    }

    fn lock_exclusive(&self) -> std::io::Result<fs::File> {
        let lock = self.open_lock()?;
        FileExt::lock_exclusive(&lock)?;
        Ok(lock)
    }

    fn open_lock(&self) -> std::io::Result<fs::File> {
        let path = self.root.join("recovery.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        validate_file(&file.metadata()?)?;
        Ok(file)
    }
}

fn validate_record_shape(record: &ComputerRemoteWorkerRecoveryRecordV1) -> std::io::Result<()> {
    if record.schema_version != 1
        || record.worker_id.is_empty()
        || record.worker_id.len() > 128
        || record.machine_id.is_empty()
        || record.session.identity.validate().is_err()
        || record.session.target.validate().is_err()
        || record.session.target.worker_id() != record.worker_id
        || record.session.target.target_machine_id() != record.machine_id
        || !uuid::Uuid::parse_str(&record.session.identity.session_id)
            .is_ok_and(|value| value.to_string() == record.session.identity.session_id)
        || (record.last_transport_sequence == 0 && !record.outcome_unknown)
        || record.last_event_sequence > 10_000_000
        || record
            .pending_action_sequence
            .is_some_and(|sequence| sequence == 0)
        || (record.pending_action_sequence.is_some() && !record.outcome_unknown)
    {
        return Err(invalid_data(
            "remote Computer Use recovery record is invalid",
        ));
    }
    Ok(())
}

fn secure_directory(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_data("recovery directory is unsafe"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(invalid_data("recovery directory owner mismatch"));
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn secure_file(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
fn validate_file(metadata: &fs::Metadata) -> std::io::Result<()> {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid_data("recovery record is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(invalid_data(
                "recovery record permissions or owner are unsafe",
            ));
        }
    }
    Ok(())
}
fn invalid_data(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn private_store_survives_reopen_and_rejects_invalid_session_paths() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let store = ComputerRemoteWorkerSessionStoreV1::open(root.path().join("private")).unwrap();
        assert!(store.load("../escape").is_err());
        assert_eq!(
            fs::metadata(root.path().join("private"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert!(store.list().unwrap().is_empty());
        drop(store);
        let reopened =
            ComputerRemoteWorkerSessionStoreV1::open(root.path().join("private")).unwrap();
        assert!(reopened.list().unwrap().is_empty());
    }
}
