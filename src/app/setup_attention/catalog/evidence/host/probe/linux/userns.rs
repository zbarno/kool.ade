use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ProbeObservationV1 as Observation,
};

use super::super::{ProbeFact, ProbeRead, ProbeSource, fact};

pub(super) fn inspect(source: &impl ProbeSource) -> ProbeFact {
    let max_user_namespaces = match source.read_text("/proc/sys/user/max_user_namespaces") {
        ProbeRead::Found(value) => match parse_integer(&value) {
            Some(value) => Some(value),
            None => return failed_user_namespaces(None, None, None),
        },
        ProbeRead::Missing => None,
        ProbeRead::Inaccessible => return inaccessible_user_namespaces(None, None, None),
        ProbeRead::Malformed => return failed_user_namespaces(None, None, None),
    };
    if max_user_namespaces == Some(0) {
        return user_namespace_fact(
            State::Unsupported,
            Code::LinuxProbeUnsupported,
            max_user_namespaces,
            None,
            None,
        );
    }
    if max_user_namespaces.is_none() {
        return user_namespace_fact(
            State::Missing,
            Code::LinuxHostCheckMissing,
            None,
            None,
            None,
        );
    }

    let unprivileged =
        optional_switch(source.read_text("/proc/sys/kernel/unprivileged_userns_clone"));
    let apparmor =
        optional_switch(source.read_text("/proc/sys/kernel/apparmor_restrict_unprivileged_userns"));
    let state_and_code = match (unprivileged, apparmor) {
        (ProbeRead::Found(0), _) => (State::Unsupported, Code::LinuxProbeUnsupported),
        (_, ProbeRead::Found(1)) => (State::Unknown, Code::HostCapabilityNotProbed),
        (ProbeRead::Malformed, _) | (_, ProbeRead::Malformed) => {
            (State::Failed, Code::LinuxProbeMalformed)
        }
        (ProbeRead::Inaccessible, _) | (_, ProbeRead::Inaccessible) => {
            (State::Unknown, Code::LinuxProbeInaccessible)
        }
        (ProbeRead::Found(1) | ProbeRead::Missing, ProbeRead::Found(0) | ProbeRead::Missing) => {
            (State::Ready, Code::LinuxHostCheckReady)
        }
        _ => (State::Unknown, Code::HostCapabilityNotProbed),
    };
    user_namespace_fact(
        state_and_code.0,
        state_and_code.1,
        max_user_namespaces,
        read_switch_value(unprivileged),
        read_switch_value(apparmor),
    )
}

pub(super) fn inspect_helpers(source: &impl ProbeSource) -> ProbeFact {
    let newuidmap = source.executable("newuidmap");
    let newgidmap = source.executable("newgidmap");
    let rootlesskit = source.executable("rootlesskit");
    let dockerd_rootless = source.executable("dockerd-rootless.sh");
    let setuptool = source.executable("dockerd-rootless-setuptool.sh");
    let slirp4netns = optional_executable(source.executable("slirp4netns"));
    let required = [
        &newuidmap,
        &newgidmap,
        &rootlesskit,
        &dockerd_rootless,
        &setuptool,
    ];
    let state_code = if required
        .iter()
        .all(|item| matches!(item, ProbeRead::Found(true)))
    {
        (State::Ready, Code::LinuxHostCheckReady)
    } else if required
        .iter()
        .any(|item| matches!(item, ProbeRead::Malformed))
    {
        (State::Failed, Code::LinuxProbeMalformed)
    } else if required
        .iter()
        .any(|item| matches!(item, ProbeRead::Inaccessible))
    {
        (State::Unknown, Code::LinuxProbeInaccessible)
    } else {
        (State::Missing, Code::LinuxHostCheckMissing)
    };
    fact(
        state_code.0,
        state_code.1,
        Some(Observation::LinuxRootlessHelpers {
            newuidmap: is_present(&newuidmap),
            newgidmap: is_present(&newgidmap),
            rootlesskit: is_present(&rootlesskit),
            dockerd_rootless: is_present(&dockerd_rootless),
            setuptool: is_present(&setuptool),
            slirp4netns,
        }),
    )
}

fn optional_switch(read: ProbeRead<String>) -> ProbeRead<u8> {
    match read {
        ProbeRead::Found(value) => match parse_integer(&value) {
            Some(value @ 0..=1) => ProbeRead::Found(value as u8),
            _ => ProbeRead::Malformed,
        },
        ProbeRead::Missing => ProbeRead::Missing,
        ProbeRead::Inaccessible => ProbeRead::Inaccessible,
        ProbeRead::Malformed => ProbeRead::Malformed,
    }
}

fn parse_integer(value: &str) -> Option<u32> {
    let value = value.trim();
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

fn read_switch_value(read: ProbeRead<u8>) -> Option<bool> {
    match read {
        ProbeRead::Found(value) => Some(value == 1),
        ProbeRead::Missing | ProbeRead::Inaccessible | ProbeRead::Malformed => None,
    }
}

fn optional_executable(read: ProbeRead<bool>) -> Option<bool> {
    match read {
        ProbeRead::Found(value) => Some(value),
        ProbeRead::Missing => Some(false),
        ProbeRead::Inaccessible | ProbeRead::Malformed => None,
    }
}

fn is_present(read: &ProbeRead<bool>) -> bool {
    matches!(read, ProbeRead::Found(true))
}

fn user_namespace_fact(
    state: State,
    code: Code,
    max_user_namespaces: Option<u32>,
    unprivileged_clone_allowed: Option<bool>,
    apparmor_userns_restricted: Option<bool>,
) -> ProbeFact {
    fact(
        state,
        code,
        Some(Observation::LinuxUserNamespaces {
            max_user_namespaces,
            unprivileged_clone_allowed,
            apparmor_userns_restricted,
        }),
    )
}

fn inaccessible_user_namespaces(
    max: Option<u32>,
    unprivileged: Option<bool>,
    apparmor: Option<bool>,
) -> ProbeFact {
    user_namespace_fact(
        State::Unknown,
        Code::LinuxProbeInaccessible,
        max,
        unprivileged,
        apparmor,
    )
}

fn failed_user_namespaces(
    max: Option<u32>,
    unprivileged: Option<bool>,
    apparmor: Option<bool>,
) -> ProbeFact {
    user_namespace_fact(
        State::Failed,
        Code::LinuxProbeMalformed,
        max,
        unprivileged,
        apparmor,
    )
}
