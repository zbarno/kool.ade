use super::super::*;
use crate::computer_use::ComputerGuestReadinessV1;

impl LocalKvmComputerBackend {
    pub(super) fn mark_guest_ready_impl(
        &self,
        session: &ComputerSessionV1,
        handle: &Handle,
        readiness: ComputerGuestReadinessV1,
        now_unix_ms: u64,
    ) -> Result<Event, Error> {
        let mut machines = self.owned_machine(session, handle, now_unix_ms)?;
        let machine = machines
            .get_mut(&handle.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if machine.owner.state != "booting"
            || readiness.session_id != handle.session_id
            || readiness.lease_generation != handle.lease_generation
            || !Uuid::parse_str(&readiness.guest_boot_id)
                .is_ok_and(|id| id.to_string() == readiness.guest_boot_id)
            || readiness.transcript_sha256.len() != 64
            || !readiness
                .transcript_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(Error::GuestReadinessUnverified);
        }
        let previous = machine.owner.clone();
        machine.owner.guest_boot_id = Some(readiness.guest_boot_id);
        machine.owner.guest_transcript_sha256 = Some(readiness.transcript_sha256);
        machine.owner.state = "connected".to_owned();
        let event = match self.event(machine, EventKind::GuestReady) {
            Ok(event) => event,
            Err(error) => {
                machine.owner = previous;
                return Err(error);
            }
        };
        if machine
            .supervision
            .compare_exchange(
                0,
                1,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            machine.owner = previous;
            return Err(Error::GuestReadinessUnverified);
        }
        Ok(event)
    }
}
