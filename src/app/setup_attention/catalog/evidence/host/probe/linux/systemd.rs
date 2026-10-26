use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ProbeObservationV1 as Observation,
};

use super::super::{EntryKind, EntryMetadata, ProbeFact, ProbeRead, ProbeSource, fact};

pub(super) fn inspect(source: &impl ProbeSource, uid: u32) -> ProbeFact {
    let runtime_path = format!("/run/user/{uid}");
    let manager_path = format!("{runtime_path}/systemd/private");
    let bus_path = format!("{runtime_path}/bus");
    let runtime = inspect_entry(
        source.metadata(&runtime_path),
        EntryKind::Directory,
        uid,
        true,
    );
    let manager = inspect_entry(
        source.metadata(&manager_path),
        EntryKind::Socket,
        uid,
        false,
    );
    let bus = inspect_entry(source.metadata(&bus_path), EntryKind::Socket, uid, false);
    let values = [runtime, manager, bus];
    let (state, code) = if values.iter().all(|item| *item == EntryState::Ready) {
        (State::Ready, Code::LinuxServiceManagerDetected)
    } else if values.contains(&EntryState::Malformed) {
        (State::Failed, Code::LinuxProbeMalformed)
    } else if values.contains(&EntryState::Inaccessible) {
        (State::Unknown, Code::LinuxProbeInaccessible)
    } else if values.contains(&EntryState::Missing) {
        (State::Missing, Code::LinuxHostCheckMissing)
    } else {
        (State::Unknown, Code::HostCapabilityNotProbed)
    };
    fact(
        state,
        code,
        Some(Observation::LinuxServiceManager {
            runtime_directory_ready: runtime == EntryState::Ready,
            private_socket_ready: manager == EntryState::Ready,
            bus_socket_ready: bus == EntryState::Ready,
        }),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryState {
    Ready,
    Missing,
    Inaccessible,
    Malformed,
}

fn inspect_entry(
    metadata: ProbeRead<EntryMetadata>,
    expected_kind: EntryKind,
    uid: u32,
    runtime_directory: bool,
) -> EntryState {
    match metadata {
        ProbeRead::Found(metadata) => {
            let mode = metadata.mode & 0o777;
            let permissions_valid = if runtime_directory {
                mode == 0o700
            } else {
                mode & 0o022 == 0
            };
            if !metadata.symlink
                && metadata.kind == expected_kind
                && metadata.owner_uid == uid
                && permissions_valid
            {
                EntryState::Ready
            } else {
                EntryState::Malformed
            }
        }
        ProbeRead::Missing => EntryState::Missing,
        ProbeRead::Inaccessible => EntryState::Inaccessible,
        ProbeRead::Malformed => EntryState::Malformed,
    }
}
