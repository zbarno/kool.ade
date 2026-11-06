use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use super::super::{
    ComputerBackendErrorV1 as Error,
    registry::{MachineOwnerRecordV1, read_owner, with_registry_lock, write_owner},
};
use super::process::{CgroupV2, terminate_recovered};

#[allow(clippy::too_many_arguments)] // Keep the watchdog inputs immutable and explicit at its thread boundary.
#[allow(clippy::collapsible_match)] // Timeout retries need side effects in the match arm, not a guard.
pub(super) fn supervise(
    owner: MachineOwnerRecordV1,
    directory: PathBuf,
    runtime_root: PathBuf,
    cgroup_root: PathBuf,
    stop_timeout: Duration,
    readiness_timeout: Duration,
    lifetime: Duration,
    state: Arc<AtomicU8>,
) {
    let booted_at = Instant::now();
    loop {
        thread::sleep(Duration::from_millis(50));
        match state.load(Ordering::Acquire) {
            2 | 4 => return,
            0 if booted_at.elapsed() >= readiness_timeout => {
                if timeout_machine(
                    &owner,
                    &directory,
                    &runtime_root,
                    &cgroup_root,
                    &stop_timeout,
                    &state,
                )
                .is_ok()
                {
                    state.store(4, Ordering::Release);
                    return;
                }
            }
            3 => {
                if timeout_machine(
                    &owner,
                    &directory,
                    &runtime_root,
                    &cgroup_root,
                    &stop_timeout,
                    &state,
                )
                .is_ok()
                {
                    state.store(4, Ordering::Release);
                    return;
                }
            }
            _ if booted_at.elapsed() >= lifetime => {
                if timeout_machine(
                    &owner,
                    &directory,
                    &runtime_root,
                    &cgroup_root,
                    &stop_timeout,
                    &state,
                )
                .is_ok()
                {
                    state.store(4, Ordering::Release);
                    return;
                }
            }
            _ => {}
        }
    }
}

fn timeout_machine(
    owner: &MachineOwnerRecordV1,
    directory: &Path,
    runtime_root: &Path,
    cgroup_root: &Path,
    stop_timeout: &Duration,
    state: &AtomicU8,
) -> Result<(), Error> {
    let current_state = state.load(Ordering::Acquire);
    if current_state != 3
        && (!matches!(current_state, 0 | 1)
            || state
                .compare_exchange(current_state, 3, Ordering::AcqRel, Ordering::Acquire)
                .is_err())
    {
        return Err(Error::InvalidTransition);
    }
    with_registry_lock(runtime_root, || {
        let mut current = match read_owner(directory) {
            Ok(current) => current,
            Err(_) => {
                CgroupV2::new(cgroup_root)?.kill_and_remove(&owner.session_id)?;
                return Ok(());
            }
        };
        if state.load(Ordering::Acquire) != 3
            || !matches!(
                current.state.as_str(),
                "booting" | "connected" | "suspended"
            )
        {
            CgroupV2::new(cgroup_root)?.kill_and_remove(&owner.session_id)?;
            return Ok(());
        }
        if current.session_id != owner.session_id
            || current.lease_id != owner.lease_id
            || current.pid != owner.pid
            || current.process_start_ticks != owner.process_start_ticks
        {
            CgroupV2::new(cgroup_root)?.kill_and_remove(&owner.session_id)?;
            return Ok(());
        }
        let _termination_attempt = owner
            .pid
            .map(|pid| terminate_recovered(pid, owner, stop_timeout));
        // cgroup removal is authoritative and succeeds only after the group is empty.
        CgroupV2::new(cgroup_root)?.kill_and_remove(&owner.session_id)?;
        current.state = "timed_out".to_owned();
        current.pid = None;
        current.process_start_ticks = None;
        current.event_sequence = current.event_sequence.saturating_add(1);
        write_owner(directory, &current)
    })
}
