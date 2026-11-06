#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerBackendErrorV1 {
    UnsupportedHost,
    HostNotReady,
    InvalidSession,
    UntrustedSession,
    UnsupportedPolicy,
    InvalidOverlay,
    DuplicateSession,
    MachineSlotLimit,
    ResourceLimit,
    UnsafeRuntimeDirectory,
    OwnershipMismatch,
    StaleLease,
    InvalidTransition,
    QemuUnavailable,
    QemuStartFailed,
    QemuExited,
    QemuTimedOut,
    GuestChannelUnavailable,
    GuestReadinessUnverified,
    SnapshotNotAllowed,
    Cancelled,
    Io,
}

impl std::fmt::Display for ComputerBackendErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "computer-use backend error: {self:?}")
    }
}

impl std::error::Error for ComputerBackendErrorV1 {}
