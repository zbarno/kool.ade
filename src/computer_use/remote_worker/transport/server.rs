use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::{
    computer_use::remote_worker::{
        ComputerRemoteWorkerHostConfigV1, ComputerRemoteWorkerOperationV1,
        ComputerRemoteWorkerResultV1,
    },
    computer_use::{ComputerBackendEventV1, ComputerMachineHandleV1, ComputerSessionV1},
};

use super::{
    ComputerRemoteWorkerCheckpointV1, ComputerRemoteWorkerRequestBodyV1 as Body,
    ComputerRemoteWorkerRequestV1, ComputerRemoteWorkerResponseV1,
    ComputerRemoteWorkerSessionFenceV1, VerifiedComputerRemoteWorkerIdentityV1,
    decode_remote_computer_worker_frame_v1, encode_remote_computer_worker_frame_v1,
};

/// Atomic per-worker session checkpoint storage. Implementations must hold a
/// host-wide lock for the serving connection and durably save before effects.
pub trait ComputerRemoteWorkerCheckpointStoreV1 {
    fn load(&mut self)
    -> io::Result<Option<(ComputerSessionV1, ComputerRemoteWorkerCheckpointV1)>>;
    fn save(
        &mut self,
        session: &ComputerSessionV1,
        checkpoint: &ComputerRemoteWorkerCheckpointV1,
    ) -> io::Result<()>;
    fn clear(&mut self) -> io::Result<()>;
}

const MAX_CHECKPOINT_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCheckpointV1 {
    schema_version: u16,
    session: ComputerSessionV1,
    checkpoint: ComputerRemoteWorkerCheckpointV1,
}

/// Private, process-exclusive checkpoint storage for the worker session fence.
/// The configured path is explicit; no default trust or state location exists.
pub struct FileComputerRemoteWorkerCheckpointStoreV1 {
    path: PathBuf,
    lock: fs::File,
}

impl FileComputerRemoteWorkerCheckpointStoreV1 {
    pub fn open(path: &Path) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| invalid_data("checkpoint path has no parent"))?;
        fs::create_dir_all(parent)?;
        secure_private_directory(parent)?;
        let lock_path = path.with_extension("lock");
        let lock = open_private_file(&lock_path)?;
        lock.lock_exclusive()?;
        Ok(Self {
            path: path.to_path_buf(),
            lock,
        })
    }
}

impl ComputerRemoteWorkerCheckpointStoreV1 for FileComputerRemoteWorkerCheckpointStoreV1 {
    fn load(
        &mut self,
    ) -> io::Result<Option<(ComputerSessionV1, ComputerRemoteWorkerCheckpointV1)>> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        validate_private_file_metadata(&metadata)?;
        if metadata.len() > MAX_CHECKPOINT_BYTES {
            return Err(invalid_data("checkpoint exceeds size limit"));
        }
        let bytes = fs::read(&self.path)?;
        let stored: StoredCheckpointV1 =
            serde_json::from_slice(&bytes).map_err(|_| invalid_data("checkpoint is malformed"))?;
        if stored.schema_version != 1 {
            return Err(invalid_data("checkpoint schema is unsupported"));
        }
        Ok(Some((stored.session, stored.checkpoint)))
    }

    fn save(
        &mut self,
        session: &ComputerSessionV1,
        checkpoint: &ComputerRemoteWorkerCheckpointV1,
    ) -> io::Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| invalid_data("checkpoint path has no parent"))?;
        secure_private_directory(parent)?;
        let bytes = serde_json::to_vec(&StoredCheckpointV1 {
            schema_version: 1,
            session: session.clone(),
            checkpoint: checkpoint.clone(),
        })
        .map_err(|_| invalid_data("checkpoint could not be serialized"))?;
        if bytes.len() as u64 > MAX_CHECKPOINT_BYTES {
            return Err(invalid_data("checkpoint exceeds size limit"));
        }
        let temp_path = parent.join(format!(".checkpoint-{}.tmp", uuid::Uuid::new_v4()));
        let mut temp = open_private_file(&temp_path)?;
        let result = (|| {
            temp.write_all(&bytes)?;
            temp.sync_all()?;
            fs::rename(&temp_path, &self.path)?;
            fs::File::open(parent)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp_path);
        }
        result
    }

    fn clear(&mut self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => fs::File::open(
                self.path
                    .parent()
                    .ok_or_else(|| invalid_data("checkpoint path has no parent"))?,
            )?
            .sync_all(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Drop for FileComputerRemoteWorkerCheckpointStoreV1 {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
    }
}

fn open_private_file(path: &Path) -> io::Result<fs::File> {
    use std::fs::OpenOptions;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            validate_private_file_metadata(&metadata)?;
            OpenOptions::new().read(true).write(true).open(path)
        }
        Err(error) => Err(error),
    }
}

fn secure_private_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(invalid_data("checkpoint directory is not a directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(invalid_data("checkpoint directory owner mismatch"));
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn validate_private_file_metadata(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.file_type().is_file() {
        return Err(invalid_data("checkpoint is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(invalid_data(
                "checkpoint file permissions or owner are unsafe",
            ));
        }
    }
    Ok(())
}

/// Worker-owned adapter around LocalKvmComputerBackend and the authenticated
/// guest channel. It receives signed sessions/actions, never controller keys.
pub trait ComputerRemoteWorkerRuntimeV1 {
    fn advertise(
        &mut self,
    ) -> io::Result<crate::computer_use::remote_worker::ComputerRemoteWorkerAdvertisementV1>;
    fn start(
        &mut self,
        session: &ComputerSessionV1,
    ) -> io::Result<(ComputerMachineHandleV1, ComputerBackendEventV1)>;
    fn operation(
        &mut self,
        session: &ComputerSessionV1,
        operation: &ComputerRemoteWorkerOperationV1,
    ) -> io::Result<ComputerRemoteWorkerResultV1>;
    fn cancel(&mut self, session: &ComputerSessionV1) -> io::Result<ComputerBackendEventV1>;
    fn recover(&mut self, session: &ComputerSessionV1) -> io::Result<()>;
    fn release(
        &mut self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
    ) -> io::Result<()>;
}

/// Serve one authenticated SSH stdio connection. The caller must already hold
/// the worker-wide exclusive lock and provide its explicit private config.
pub fn serve_computer_remote_worker_stdio_v1<R, W, S, V>(
    mut input: R,
    mut output: W,
    mut store: S,
    mut runtime: V,
    config: &ComputerRemoteWorkerHostConfigV1,
) -> io::Result<()>
where
    R: Read,
    W: Write,
    S: ComputerRemoteWorkerCheckpointStoreV1,
    V: ComputerRemoteWorkerRuntimeV1,
{
    let controller_key = config.validate().map_err(invalid_data)?;
    let identity = VerifiedComputerRemoteWorkerIdentityV1::from_host_config(config);
    let now = now_unix_ms()?;
    let mut fence = store
        .load()?
        .map(|(session, checkpoint)| {
            ComputerRemoteWorkerSessionFenceV1::restore(
                session,
                checkpoint,
                config,
                &identity,
                &config.controller_key_id,
                &controller_key,
                now,
            )
            .map_err(invalid_data)
        })
        .transpose()?;

    loop {
        let Some(request) = read_request(&mut input)? else {
            return Ok(());
        };
        request.validate_shape().map_err(invalid_data)?;
        if matches!(&request.body, Body::Probe) {
            let response = ComputerRemoteWorkerResponseV1 {
                protocol_version: request.protocol_version,
                worker_id: request.worker_id.clone(),
                session_id: request.session_id.clone(),
                lease_id: request.lease_id.clone(),
                lease_generation: request.lease_generation,
                transport_sequence: request.transport_sequence,
                result: ComputerRemoteWorkerResultV1::Advertisement(runtime.advertise()?),
            };
            response.validate().map_err(invalid_data)?;
            output.write_all(&encode_remote_computer_worker_frame_v1(&response)?)?;
            output.flush()?;
            continue;
        }
        let (session, result) = match (&mut fence, &request.body) {
            (None, Body::Start { session }) => {
                let started = ComputerRemoteWorkerSessionFenceV1::accept_start(
                    &request,
                    config,
                    &identity,
                    &config.controller_key_id,
                    &controller_key,
                    now_unix_ms()?,
                )
                .map_err(invalid_data)?;
                store.save(session, &started.checkpoint())?;
                let (handle, event) = runtime.start(session)?;
                fence = Some(started);
                (
                    session.clone(),
                    ComputerRemoteWorkerResultV1::Prepared { handle, event },
                )
            }
            (Some(_), _) => {
                let mut current = fence.take().expect("fence exists");
                current
                    .accept_request(
                        &request,
                        &config.controller_key_id,
                        &controller_key,
                        now_unix_ms()?,
                    )
                    .map_err(invalid_data)?;
                let session = current.session().clone();
                store.save(&session, &current.checkpoint())?;
                let result = match &request.body {
                    Body::Operation { operation } => {
                        let result = runtime.operation(&session, operation)?;
                        if matches!(operation, ComputerRemoteWorkerOperationV1::Release { .. }) {
                            store.clear()?;
                        }
                        if let (
                            ComputerRemoteWorkerOperationV1::GuestAction { action, .. },
                            ComputerRemoteWorkerResultV1::GuestAction(outcome),
                        ) = (operation, &result)
                        {
                            current
                                .record_guest_action_outcome(outcome)
                                .map_err(invalid_data)?;
                            store.save(&session, &current.checkpoint())?;
                            if outcome.reply.action_sequence != action.action_sequence {
                                return Err(invalid_data(
                                    "worker guest action sequence mismatched",
                                ));
                            }
                        }
                        result
                    }
                    Body::Action { action } => {
                        let result = runtime.operation(
                            &session,
                            &ComputerRemoteWorkerOperationV1::GuestAction {
                                handle: ComputerMachineHandleV1::from_remote(
                                    session.identity.session_id.clone(),
                                    session.lease.lease_id.clone(),
                                    session.lease.generation,
                                    session.target.target_machine_id().into(),
                                ),
                                action: action.clone(),
                                deadline_unix_ms: session.lease.expires_at_unix_ms,
                            },
                        )?;
                        let ComputerRemoteWorkerResultV1::GuestAction(outcome) = &result else {
                            return Err(invalid_data("worker action response has wrong type"));
                        };
                        current
                            .record_guest_action_outcome(outcome)
                            .map_err(invalid_data)?;
                        store.save(&session, &current.checkpoint())?;
                        result
                    }
                    Body::Cancel { .. } => {
                        let mut event = runtime.cancel(&session)?;
                        event.lease_generation = session.lease.generation;
                        ComputerRemoteWorkerResultV1::BackendEvent(event)
                    }
                    Body::Recover { .. } => {
                        runtime.recover(&session)?;
                        ComputerRemoteWorkerResultV1::Acknowledged
                    }
                    Body::Start { .. } => unreachable!("replayed start is rejected by fence"),
                    Body::Probe => unreachable!("probe is handled before the session fence"),
                };
                fence = Some(current);
                (session, result)
            }
            (None, _) => {
                return Err(invalid_data(
                    "worker must receive Start before other requests",
                ));
            }
        };
        let response = ComputerRemoteWorkerResponseV1 {
            protocol_version: request.protocol_version,
            worker_id: request.worker_id,
            session_id: session.identity.session_id,
            lease_id: session.lease.lease_id,
            lease_generation: session.lease.generation,
            transport_sequence: request.transport_sequence,
            result,
        };
        response.validate().map_err(invalid_data)?;
        output.write_all(&encode_remote_computer_worker_frame_v1(&response)?)?;
        output.flush()?;
    }
}

fn read_request<R: Read>(input: &mut R) -> io::Result<Option<ComputerRemoteWorkerRequestV1>> {
    let mut prefix = [0; 4];
    let mut read = 0;
    while read < 4 {
        match input.read(&mut prefix[read..])? {
            0 if read == 0 => return Ok(None),
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated frame prefix",
                ));
            }
            n => read += n,
        }
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > super::MAX_REMOTE_COMPUTER_WORKER_FRAME_BYTES_V1 {
        return Err(invalid_data("worker request exceeds frame limit"));
    }
    let mut frame = Vec::with_capacity(length + 4);
    frame.extend_from_slice(&prefix);
    frame.resize(length + 4, 0);
    input.read_exact(&mut frame[4..])?;
    decode_remote_computer_worker_frame_v1(&frame).map(Some)
}

fn now_unix_ms() -> io::Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .map_err(|_| io::Error::other("system clock is before Unix epoch"))
}
fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_store_is_private_locked_and_rejects_corrupt_state() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("worker/checkpoint.json");
        let mut store = FileComputerRemoteWorkerCheckpointStoreV1::open(&path).unwrap();
        assert_eq!(store.load().unwrap(), None);
        let lock_path = path.with_extension("lock");
        assert_eq!(
            fs::metadata(lock_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let second_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        assert!(FileExt::try_lock_exclusive(&second_lock).is_err());
        drop(second_lock);

        fs::write(&path, b"not-json").unwrap();
        assert_eq!(store.load().unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
