use super::*;

pub(super) fn event(
    session: &ComputerSessionV1,
    sequence: u64,
    kind: ComputerBackendEventKindV1,
) -> crate::computer_use::ComputerBackendEventV1 {
    crate::computer_use::ComputerBackendEventV1 {
        sequence,
        session_id: session.identity.session_id.clone(),
        lease_generation: session.lease.generation,
        kind,
        boot_fingerprint: None,
    }
}
