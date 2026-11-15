//! Windows guest-control transport gates.
//!
//! QEMU's built-in Windows `pipe` chardev creates its own pipe with a default
//! security descriptor and does not request `PIPE_REJECT_REMOTE_CLIENTS`.
//! Therefore this implementation deliberately refuses to connect to it. An
//! app-owned AF_UNIX listener now verifies the peer PID, but startup remains
//! blocked until a trusted QEMU client build passes native integration checks.

use std::io;

#[cfg(windows)]
#[path = "transport/af_unix.rs"]
mod af_unix;

#[cfg(windows)]
pub(crate) use af_unix::WindowsAfUnixListener;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WindowsQemuAfUnixEvidence {
    pub(crate) trusted_qemu_build: Option<bool>,
    pub(crate) qemu_client_mode_verified: Option<bool>,
    pub(crate) active_child_peer_pid_verified: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowsQemuAfUnixBlocker {
    TrustedBuildNotPinned,
    ClientModeNotVerified,
    PeerPidNotVerified,
}

pub(crate) fn assess_qemu_af_unix_evidence(
    evidence: WindowsQemuAfUnixEvidence,
) -> Result<(), WindowsQemuAfUnixBlocker> {
    if evidence.trusted_qemu_build != Some(true) {
        return Err(WindowsQemuAfUnixBlocker::TrustedBuildNotPinned);
    }
    if evidence.qemu_client_mode_verified != Some(true) {
        return Err(WindowsQemuAfUnixBlocker::ClientModeNotVerified);
    }
    if evidence.active_child_peer_pid_verified != Some(true) {
        return Err(WindowsQemuAfUnixBlocker::PeerPidNotVerified);
    }
    Ok(())
}

/// No trusted Windows QEMU build has yet passed native AF_UNIX client tests.
/// The listener implementation is therefore not permission to start a VM.
pub(crate) fn require_qemu_af_unix_evidence() -> io::Result<()> {
    assess_qemu_af_unix_evidence(WindowsQemuAfUnixEvidence::default()).map_err(|blocker| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("Windows QEMU AF_UNIX transport is unavailable: {blocker:?}"),
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

    #[test]
    fn qemu_af_unix_startup_requires_a_trusted_native_client_and_verified_peer_pid() {
        let complete = WindowsQemuAfUnixEvidence {
            trusted_qemu_build: Some(true),
            qemu_client_mode_verified: Some(true),
            active_child_peer_pid_verified: Some(true),
        };
        assert_eq!(assess_qemu_af_unix_evidence(complete), Ok(()));
        for missing in [
            WindowsQemuAfUnixEvidence {
                trusted_qemu_build: None,
                ..complete
            },
            WindowsQemuAfUnixEvidence {
                qemu_client_mode_verified: None,
                ..complete
            },
            WindowsQemuAfUnixEvidence {
                active_child_peer_pid_verified: None,
                ..complete
            },
        ] {
            assert!(assess_qemu_af_unix_evidence(missing).is_err());
        }
        assert!(require_qemu_af_unix_evidence().is_err());
    }
}
