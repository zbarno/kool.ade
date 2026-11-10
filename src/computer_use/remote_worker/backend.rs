//! Controller-side `ComputerBackend` adapter over the pinned worker stream.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use ed25519_dalek::VerifyingKey;
use sha2::Digest;

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionV1, ComputerBackend, ComputerBackendErrorV1 as Error,
    ComputerBackendEventV1, ComputerBackendProbeV1, ComputerDisplayFrameV1, ComputerDisplayInputV1,
    ComputerGuestChannelV1, ComputerGuestReadinessV1, ComputerMachineHandleV1,
    ComputerMachineRecordV1, ComputerSessionV1,
    guest_image::GuestImageOverlayV1,
    guest_protocol::{
        AuthenticatedGuestChannelV1, GuestActionJournalV1, GuestActionOutcomeV1,
        GuestActionTransportV1, GuestAuthenticatedContextV1, GuestProtocolErrorV1,
    },
};

use super::{
    ComputerRemoteWorkerAdvertisementV1, ComputerRemoteWorkerClientV1,
    ComputerRemoteWorkerEnrollmentV1, ComputerRemoteWorkerOperationV1 as Operation,
    ComputerRemoteWorkerRecoveryRecordV1, ComputerRemoteWorkerRequestBodyV1 as Body,
    ComputerRemoteWorkerRequestV1 as Request, ComputerRemoteWorkerRequirementsV1,
    ComputerRemoteWorkerResultV1 as ResultValue, ComputerRemoteWorkerSessionStoreV1,
    RemoteComputerWorkerEligibilityV1, assess_remote_computer_worker_v1,
};

struct ActiveRemoteSession {
    session: ComputerSessionV1,
    handle: ComputerMachineHandleV1,
    client: ComputerRemoteWorkerClientV1,
    last_event_sequence: u64,
}

type SharedSessions = Arc<Mutex<HashMap<String, ActiveRemoteSession>>>;

/// Controller backend for one separately enrolled worker. It never sends the
/// caller's local overlay path; the worker resolves the digest in its own
/// signed catalog and private image cache.
pub struct RemoteComputerBackendV1 {
    enrollment: ComputerRemoteWorkerEnrollmentV1,
    ssh: crate::execution::remote::ssh::SshWorkerConfig,
    controller_key_id: String,
    controller_key: VerifyingKey,
    journal_root: PathBuf,
    sessions: SharedSessions,
    recovery_store: super::ComputerRemoteWorkerSessionStoreV1,
}

struct RemoteGuestActionTransport {
    enrollment: ComputerRemoteWorkerEnrollmentV1,
    sessions: SharedSessions,
    recovery_store: super::ComputerRemoteWorkerSessionStoreV1,
}
impl GuestActionTransportV1 for RemoteGuestActionTransport {
    fn execute_action(
        &self,
        session: &ComputerSessionV1,
        _actor: ComputerActionActorV1,
        action: &ComputerActionV1,
        deadline: u64,
        _now: u64,
    ) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
        if self
            .recovery_store
            .load(&session.identity.session_id)
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?
            .is_none_or(|record| record.pending_action_sequence.is_some())
        {
            return Err(GuestProtocolErrorV1::OutcomeUnknown);
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        let active = sessions
            .get_mut(&session.identity.session_id)
            .ok_or(GuestProtocolErrorV1::OutcomeUnknown)?;
        if active.session.lease.generation != session.lease.generation {
            return Err(GuestProtocolErrorV1::StaleLease);
        }
        let request = Request {
            protocol_version: self.enrollment.transport_protocol_version,
            worker_id: self.enrollment.worker_id.clone(),
            ssh_host_key_fingerprint: self.enrollment.ssh_host_key_fingerprint.clone(),
            session_id: session.identity.session_id.clone(),
            lease_id: session.lease.lease_id.clone(),
            lease_generation: session.lease.generation,
            approval_generation: session.approval_generation,
            cancellation_generation: session.cancellation_generation,
            transport_sequence: active.client.next_transport_sequence(),
            request_id: uuid::Uuid::new_v4().to_string(),
            body: Body::Operation {
                operation: Operation::GuestAction {
                    handle: active.handle.clone(),
                    action: action.clone(),
                    deadline_unix_ms: deadline,
                },
            },
        };
        let mut recovery = self
            .recovery_store
            .load(&session.identity.session_id)
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?
            .ok_or(GuestProtocolErrorV1::OutcomeUnknown)?;
        recovery.last_transport_sequence = request.transport_sequence;
        recovery.pending_action_sequence = Some(action.action_sequence);
        recovery.outcome_unknown = true;
        self.recovery_store
            .save(&recovery)
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        let response = active
            .client
            .request(&request)
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        match response.result {
            ResultValue::GuestAction(outcome) => Ok(outcome),
            _ => Err(GuestProtocolErrorV1::OutcomeUnknown),
        }
    }

    fn acknowledge_action_outcome(
        &self,
        session: &ComputerSessionV1,
        action_sequence: u64,
    ) -> Result<(), GuestProtocolErrorV1> {
        let mut record = self
            .recovery_store
            .load(&session.identity.session_id)
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?
            .ok_or(GuestProtocolErrorV1::OutcomeUnknown)?;
        if record.session.lease.lease_id != session.lease.lease_id
            || record.session.lease.generation != session.lease.generation
            || record
                .pending_action_sequence
                .is_some_and(|pending| pending != action_sequence)
        {
            return Err(GuestProtocolErrorV1::StaleLease);
        }
        record.pending_action_sequence = None;
        record.outcome_unknown = false;
        self.recovery_store
            .save(&record)
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)
    }
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn valid_artifact_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('.')
        && !value.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        && !value.chars().any(char::is_control)
        && !matches!(value, "." | "..")
}
fn now_ms() -> Result<u64, Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .map_err(|_| Error::Io)
}
fn ensure_private_dir(path: &std::path::Path) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|_| Error::Io)?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Error::UnsafeRuntimeDirectory)?;
    if !metadata.file_type().is_dir() {
        return Err(Error::UnsafeRuntimeDirectory);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(Error::UnsafeRuntimeDirectory);
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Io)?;
    }
    Ok(())
}

mod computer_backend;
mod controller;
#[cfg(all(test, target_os = "linux"))]
mod tests;
