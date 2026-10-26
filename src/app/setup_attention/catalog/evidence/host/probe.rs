use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    ImageArchitectureV1 as Architecture, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence,
    ProbeObservationV1 as Observation,
};

mod distro;
mod linux;
#[cfg(target_os = "linux")]
mod system;

#[cfg(test)]
pub(super) use distro::distribution_from_os_release;
pub(super) use linux::{LinuxProbeFacts, inspect as inspect_linux};
#[cfg(test)]
pub(super) use linux::{parse_distribution, parse_kernel};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProbeRead<T> {
    Found(T),
    Missing,
    Inaccessible,
    Malformed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EntryKind {
    Directory,
    Socket,
    RegularFile,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct EntryMetadata {
    pub kind: EntryKind,
    pub owner_uid: u32,
    pub mode: u32,
    pub symlink: bool,
}

pub(super) trait ProbeSource {
    fn read_text(&self, path: &str) -> ProbeRead<String>;
    fn metadata(&self, path: &str) -> ProbeRead<EntryMetadata>;
    fn executable(&self, name: &str) -> ProbeRead<bool>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProbeFact {
    pub state: State,
    pub code: Code,
    pub observation: Option<Observation>,
}

pub(super) fn fact(state: State, code: Code, observation: Option<Observation>) -> ProbeFact {
    ProbeFact {
        state,
        code,
        observation,
    }
}

pub(super) fn collect(host: HostKind, architecture: Architecture) -> LinuxProbeFacts {
    #[cfg(target_os = "linux")]
    {
        if host != HostKind::Linux {
            return LinuxProbeFacts::not_applicable();
        }
        let uid = unsafe { libc::geteuid() };
        inspect_linux(
            &system::SystemProbeSource,
            uid,
            current_username(uid),
            architecture,
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (host, architecture);
        LinuxProbeFacts::not_applicable()
    }
}

#[cfg(target_os = "linux")]
fn current_username(uid: u32) -> Option<String> {
    match system::SystemProbeSource.read_text("/etc/passwd") {
        ProbeRead::Found(contents) => contents.lines().find_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let entry_uid = fields.next()?.parse::<u32>().ok()?;
            (entry_uid == uid).then(|| name.to_owned())
        }),
        ProbeRead::Missing | ProbeRead::Inaccessible | ProbeRead::Malformed => None,
    }
}

pub(super) fn push_evidence(
    result: &mut Vec<Evidence>,
    host: HostKind,
    generation: u64,
    now: u64,
    facts: &LinuxProbeFacts,
) {
    for (capability, probe_fact) in [
        (Capability::LinuxRootlessRecipe, &facts.recipe),
        (Capability::LinuxKernel, &facts.kernel),
        (Capability::LinuxUserNamespaces, &facts.user_namespaces),
        (Capability::LinuxSubuid, &facts.subuid),
        (Capability::LinuxSubgid, &facts.subgid),
        (Capability::LinuxRootlessHelpers, &facts.helpers),
        (Capability::LinuxCgroups, &facts.cgroups),
        (Capability::LinuxUidMappings, &facts.uid_mappings),
        (Capability::LinuxServiceManager, &facts.service_manager),
        (Capability::LinuxPackageManager, &facts.package_manager),
    ] {
        let mut item = crate::app::setup_attention::catalog::evidence::evidence(
            crate::app::setup_attention::catalog::PrerequisiteId {
                domain: Domain::Host,
                platform: host,
                capability,
                subject: Subject::None,
            },
            probe_fact.state,
            probe_fact.code,
            Origin::HostRuntime,
            generation,
            None,
            now,
        );
        item.observation = probe_fact.observation.clone();
        result.push(item);
    }
}

pub(super) fn state_code(state: State) -> Code {
    match state {
        State::Ready => Code::LinuxHostCheckReady,
        State::Missing => Code::LinuxHostCheckMissing,
        State::Unknown | State::Stale | State::Blocked | State::NotApplicable => {
            Code::HostCapabilityNotProbed
        }
        State::Failed => Code::LinuxProbeMalformed,
        State::Unsupported => Code::LinuxProbeUnsupported,
    }
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
