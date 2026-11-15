//! Windows guest-control transport gate.
//!
//! QEMU's built-in Windows `pipe` chardev creates its own pipe with a default
//! security descriptor and does not request `PIPE_REJECT_REMOTE_CLIENTS`.
//! Therefore this implementation deliberately refuses to connect to it.

use std::io;

use crate::computer_use::{ComputerGuestChannelV1, backend::GuestChannelIo};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowsPipeBlocker {
    PipeAclNotVerified,
    RemoteClientsNotRejected,
    ServerProcessNotVerified,
    ServerOwnerNotVerified,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WindowsPipeEvidence {
    pub(crate) private_per_user_acl: Option<bool>,
    pub(crate) remote_clients_rejected: Option<bool>,
    pub(crate) server_process_matches_job: Option<bool>,
    pub(crate) server_owner_matches_current_user: Option<bool>,
}

pub(crate) fn assess_pipe_evidence(
    evidence: WindowsPipeEvidence,
) -> Result<(), WindowsPipeBlocker> {
    if evidence.private_per_user_acl != Some(true) {
        return Err(WindowsPipeBlocker::PipeAclNotVerified);
    }
    if evidence.remote_clients_rejected != Some(true) {
        return Err(WindowsPipeBlocker::RemoteClientsNotRejected);
    }
    if evidence.server_process_matches_job != Some(true) {
        return Err(WindowsPipeBlocker::ServerProcessNotVerified);
    }
    if evidence.server_owner_matches_current_user != Some(true) {
        return Err(WindowsPipeBlocker::ServerOwnerNotVerified);
    }
    Ok(())
}

/// Current QEMU's built-in Windows pipe backend creates the server with a
/// NULL security descriptor and accepts remote clients. Its transport cannot
/// pass `assess_pipe_evidence`, so VM startup remains blocked before connect.
pub(crate) fn require_qemu_pipe_security() -> io::Result<()> {
    let evidence = WindowsPipeEvidence {
        private_per_user_acl: Some(false),
        remote_clients_rejected: Some(false),
        server_process_matches_job: None,
        server_owner_matches_current_user: None,
    };
    assess_pipe_evidence(evidence).map_err(|blocker| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("QEMU guest pipe is unavailable: {blocker:?}"),
        )
    })
}

/// Shared constructor for a future transport that has already established
/// per-user ACL, local-only, QEMU-process, and peer-owner guarantees. The
/// existing guest protocol then performs the cryptographic boot handshake.
#[allow(dead_code)]
pub(crate) fn channel_from_verified_io(
    session_id: String,
    lease_generation: u64,
    io: impl GuestChannelIo + 'static,
) -> ComputerGuestChannelV1 {
    ComputerGuestChannelV1::from_io(session_id, lease_generation, io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_default_named_pipe_is_rejected_before_connection() {
        assert_eq!(
            assess_pipe_evidence(WindowsPipeEvidence {
                private_per_user_acl: Some(false),
                remote_clients_rejected: Some(false),
                ..WindowsPipeEvidence::default()
            }),
            Err(WindowsPipeBlocker::PipeAclNotVerified)
        );
        assert_eq!(
            assess_pipe_evidence(WindowsPipeEvidence {
                private_per_user_acl: Some(true),
                remote_clients_rejected: Some(false),
                server_process_matches_job: Some(true),
                server_owner_matches_current_user: Some(true),
            }),
            Err(WindowsPipeBlocker::RemoteClientsNotRejected)
        );
        assert!(require_qemu_pipe_security().is_err());
    }

    #[test]
    fn pipe_policy_requires_acl_local_only_peer_pid_and_peer_owner_evidence() {
        let complete = WindowsPipeEvidence {
            private_per_user_acl: Some(true),
            remote_clients_rejected: Some(true),
            server_process_matches_job: Some(true),
            server_owner_matches_current_user: Some(true),
        };
        assert_eq!(assess_pipe_evidence(complete), Ok(()));
        for missing in [
            WindowsPipeEvidence {
                private_per_user_acl: None,
                ..complete
            },
            WindowsPipeEvidence {
                remote_clients_rejected: None,
                ..complete
            },
            WindowsPipeEvidence {
                server_process_matches_job: None,
                ..complete
            },
            WindowsPipeEvidence {
                server_owner_matches_current_user: None,
                ..complete
            },
        ] {
            assert!(assess_pipe_evidence(missing).is_err());
        }
    }
}
