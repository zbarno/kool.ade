use std::collections::BTreeSet;

use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ProbeObservationV1 as Observation,
};

use super::super::{ProbeFact, ProbeRead, ProbeSource, fact};

const REQUIRED_CONTROLLERS: [&str; 3] = ["cpu", "memory", "pids"];

pub(super) fn inspect(source: &impl ProbeSource, uid: u32) -> ProbeFact {
    let root = match source.read_text("/sys/fs/cgroup/cgroup.controllers") {
        ProbeRead::Found(value) => match parse_controllers(&value) {
            Some(controllers) => controllers,
            None => {
                return result(
                    State::Failed,
                    Code::LinuxProbeMalformed,
                    false,
                    false,
                    false,
                    false,
                );
            }
        },
        ProbeRead::Missing => {
            return result(
                State::Unsupported,
                Code::LinuxProbeUnsupported,
                false,
                false,
                false,
                false,
            );
        }
        ProbeRead::Inaccessible => {
            return result(
                State::Unknown,
                Code::LinuxProbeInaccessible,
                false,
                false,
                false,
                false,
            );
        }
        ProbeRead::Malformed => {
            return result(
                State::Failed,
                Code::LinuxProbeMalformed,
                false,
                false,
                false,
                false,
            );
        }
    };
    if !REQUIRED_CONTROLLERS.iter().all(|item| root.contains(*item)) {
        return result(
            State::Unsupported,
            Code::LinuxProbeUnsupported,
            true,
            false,
            false,
            false,
        );
    }

    let user_scope = format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service");
    let available_path = format!("{user_scope}/cgroup.controllers");
    let delegated_path = format!("{user_scope}/cgroup.subtree_control");
    let available = match source.read_text(&available_path) {
        ProbeRead::Found(value) => match parse_controllers(&value) {
            Some(controllers) => controllers,
            None => {
                return result(
                    State::Failed,
                    Code::LinuxProbeMalformed,
                    true,
                    false,
                    false,
                    false,
                );
            }
        },
        ProbeRead::Missing => {
            return result(
                State::Unknown,
                Code::HostCapabilityNotProbed,
                true,
                false,
                false,
                false,
            );
        }
        ProbeRead::Inaccessible => {
            return result(
                State::Unknown,
                Code::LinuxProbeInaccessible,
                true,
                false,
                false,
                false,
            );
        }
        ProbeRead::Malformed => {
            return result(
                State::Failed,
                Code::LinuxProbeMalformed,
                true,
                false,
                false,
                false,
            );
        }
    };
    if !REQUIRED_CONTROLLERS
        .iter()
        .all(|item| available.contains(*item))
    {
        return result(
            State::Missing,
            Code::LinuxHostCheckMissing,
            true,
            false,
            false,
            false,
        );
    }
    let delegated = match source.read_text(&delegated_path) {
        ProbeRead::Found(value) => match parse_controllers(&value) {
            Some(controllers) => controllers,
            None => {
                return result(
                    State::Failed,
                    Code::LinuxProbeMalformed,
                    true,
                    false,
                    false,
                    false,
                );
            }
        },
        ProbeRead::Missing => {
            return result(
                State::Unknown,
                Code::HostCapabilityNotProbed,
                true,
                false,
                false,
                false,
            );
        }
        ProbeRead::Inaccessible => {
            return result(
                State::Unknown,
                Code::LinuxProbeInaccessible,
                true,
                false,
                false,
                false,
            );
        }
        ProbeRead::Malformed => {
            return result(
                State::Failed,
                Code::LinuxProbeMalformed,
                true,
                false,
                false,
                false,
            );
        }
    };
    let cpu = delegated.contains("cpu");
    let memory = delegated.contains("memory");
    let pids = delegated.contains("pids");
    let (state, code) = if cpu && memory && pids {
        (State::Ready, Code::LinuxHostCheckReady)
    } else {
        (State::Missing, Code::LinuxHostCheckMissing)
    };
    result(state, code, true, cpu, memory, pids)
}

fn parse_controllers(contents: &str) -> Option<BTreeSet<String>> {
    let tokens = contents
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if tokens.iter().any(|token| {
        token.is_empty()
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    }) {
        None
    } else {
        Some(tokens)
    }
}

fn result(
    state: State,
    code: Code,
    cgroup_v2: bool,
    cpu_delegated: bool,
    memory_delegated: bool,
    pids_delegated: bool,
) -> ProbeFact {
    fact(
        state,
        code,
        Some(Observation::LinuxCgroups {
            cgroup_v2,
            cpu_delegated,
            memory_delegated,
            pids_delegated,
        }),
    )
}
