use crate::{
    execution::contract::ReadinessState,
    harness::discovery::{ModelCatalogStatus, ProbeResult, ProbeStatus},
};

use super::{evidence, id};
use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, HostKind,
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence, ProviderIdV1,
};

const PROVIDERS: [(&str, ProviderIdV1); 6] = [
    ("pi", ProviderIdV1::Pi),
    ("codex", ProviderIdV1::Codex),
    ("claude", ProviderIdV1::Claude),
    ("opencode", ProviderIdV1::OpenCode),
    ("copilot", ProviderIdV1::Copilot),
    ("antigravity", ProviderIdV1::Antigravity),
];

fn installed(probe: &ProbeResult, generation: u64) -> (State, Code) {
    if probe.run_generation != generation {
        (State::Stale, Code::StaleProbeGeneration)
    } else if probe.diagnostic_code
        == Some(crate::harness::discovery::ProbeDiagnosticCode::ExecutableInvalid)
    {
        (State::Failed, Code::ProviderPathInvalid)
    } else if probe.binary_path.is_some() {
        (State::Ready, Code::ProviderInstalled)
    } else if probe.status == ProbeStatus::Failed {
        (State::Failed, Code::ProviderProbeFailed)
    } else if probe.status == ProbeStatus::Missing {
        (State::Missing, Code::ProviderMissing)
    } else {
        (State::Unknown, Code::ProviderProbeFailed)
    }
}

pub(super) fn push(
    result: &mut Vec<Evidence>,
    probes: &[ProbeResult],
    host: HostKind,
    generation: u64,
    now: u64,
) {
    for (name, provider) in PROVIDERS {
        let Some(probe) = probes.iter().find(|probe| probe.provider_id == name) else {
            continue;
        };
        let subject = Subject::Provider(provider);
        let at = if probe.started_at_unix_ms == 0 {
            now
        } else {
            probe.started_at_unix_ms
        };
        let stale = probe.run_generation != generation;
        let add = |capability, state, code| {
            evidence(
                id(Domain::Provider, host, capability, subject.clone()),
                state,
                code,
                Origin::ProviderCli,
                generation,
                Some(probe.run_generation),
                at,
            )
        };
        let (state, code) = installed(probe, generation);
        result.push(add(Capability::ProviderExecutable, state, code));
        let (state, code) = if stale {
            (State::Stale, Code::StaleProbeGeneration)
        } else if probe.diagnostic_code
            == Some(crate::harness::discovery::ProbeDiagnosticCode::UnsupportedVersion)
        {
            (State::Failed, Code::ProviderVersionUnsupported)
        } else if probe.version.is_some() {
            (State::Ready, Code::ProviderVersionDetected)
        } else if probe.status == ProbeStatus::Failed {
            (State::Failed, Code::ProviderProbeFailed)
        } else {
            (State::Unknown, Code::ProviderVersionUnknown)
        };
        result.push(add(Capability::ProviderVersion, state, code));
        let (state, code) = if stale {
            (State::Stale, Code::StaleProbeGeneration)
        } else {
            match probe.authentication {
                ReadinessState::Ready => (State::Ready, Code::AuthenticationVerified),
                ReadinessState::Unavailable => (State::Missing, Code::AuthenticationRequired),
                ReadinessState::Unknown => (State::Unknown, Code::AuthenticationUnknown),
            }
        };
        result.push(add(Capability::ProviderAuthentication, state, code));
        let (state, code) = if stale {
            (State::Stale, Code::StaleProbeGeneration)
        } else {
            match probe.model_catalog_status {
                ModelCatalogStatus::Fresh
                    if !probe.models.is_empty() || probe.default_model.is_some() =>
                {
                    (State::Ready, Code::ModelCatalogVerified)
                }
                ModelCatalogStatus::Stale => (State::Stale, Code::ModelCatalogStale),
                ModelCatalogStatus::Unavailable
                | ModelCatalogStatus::NotChecked
                | ModelCatalogStatus::Fresh => (State::Unknown, Code::ModelCatalogUnavailable),
            }
        };
        result.push(add(Capability::ProviderModels, state, code));
        let (state, code) = if stale {
            (State::Stale, Code::StaleProbeGeneration)
        } else {
            match probe.execution_capability.safe_for_planning {
                ReadinessState::Ready => (State::Ready, Code::ContextOnlyVerified),
                ReadinessState::Unavailable => (State::Missing, Code::ContextOnlyUnavailable),
                ReadinessState::Unknown => (State::Unknown, Code::ContextOnlyUnknown),
            }
        };
        result.push(add(Capability::ProviderContextOnly, state, code));
    }
}
