use std::time::Duration;

use super::{evidence, id};
use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, GitProbeV1, HostKind,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence,
};

pub(crate) fn probe() -> GitProbeV1 {
    let arguments = ["git".to_owned(), "--version".to_owned()];
    let executable = match crate::harness::discovery::run_isolated(
        &arguments,
        Duration::from_secs(3),
        256,
        256,
        &[],
    ) {
        Ok(output) if output.success => (State::Ready, Code::GitAvailable),
        Ok(_) => (State::Failed, Code::GitProbeFailed),
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io_error| io_error.kind() == std::io::ErrorKind::NotFound)
            }) =>
        {
            (State::Missing, Code::GitMissing)
        }
        Err(_) => (State::Failed, Code::GitProbeFailed),
    };
    let identity = if executable.0 == State::Ready {
        probe_identity()
    } else {
        (State::Blocked, Code::ParentPrerequisiteUnavailable)
    };
    GitProbeV1 {
        executable_state: executable.0,
        executable_code: executable.1,
        identity_state: identity.0,
        identity_code: identity.1,
    }
}

fn probe_identity() -> (State, Code) {
    for key in ["user.name", "user.email"] {
        let arguments = [
            "git".to_owned(),
            "config".to_owned(),
            "--global".to_owned(),
            "--get".to_owned(),
            key.to_owned(),
        ];
        match crate::harness::discovery::run(&arguments, Duration::from_secs(3), 512, 512, &[]) {
            Ok(output)
                if output.success
                    && !output.stdout_truncated
                    && !String::from_utf8_lossy(&output.stdout).trim().is_empty() => {}
            Ok(_) => return (State::Missing, Code::GitIdentityMissing),
            Err(_) => return (State::Failed, Code::GitIdentityProbeFailed),
        }
    }
    (State::Ready, Code::GitIdentityConfigured)
}

pub(super) fn push(
    result: &mut Vec<Evidence>,
    host: HostKind,
    generation: u64,
    now: u64,
    probe: GitProbeV1,
) {
    result.push(evidence(
        id(Domain::Git, host, Capability::GitExecutable, Subject::None),
        probe.executable_state,
        probe.executable_code,
        Origin::GitCli,
        generation,
        None,
        now,
    ));
    result.push(evidence(
        id(Domain::Git, host, Capability::GitIdentity, Subject::None),
        probe.identity_state,
        probe.identity_code,
        Origin::GitCli,
        generation,
        None,
        now,
    ));
}
