use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ImageArchitectureV1 as Architecture,
    ProbeObservationV1 as Observation, WindowsEditionV1 as Edition,
    WindowsKernelVersionV1 as KernelVersion, WindowsProductV1 as Product,
};

use super::super::{ProbeFact, ProbeRead, fact};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OptionalFeatureState {
    Enabled,
    Disabled,
    Pending,
}

pub(super) fn host_fact(
    product: Product,
    edition: Edition,
    build_number: u32,
    architecture: Architecture,
) -> ProbeFact {
    let supported_product = product == Product::Windows11;
    let supported_edition = !matches!(edition, Edition::Server | Edition::Other);
    let supported_build = match product {
        Product::Windows10 => false,
        Product::Windows11 => build_number >= 22_000,
        Product::Server | Product::Unsupported => false,
    };
    let supported_architecture = architecture == Architecture::Amd64;
    let state =
        if supported_product && supported_edition && supported_build && supported_architecture {
            State::Ready
        } else if edition == Edition::Other {
            State::Unknown
        } else {
            State::Unsupported
        };
    let code = if state == State::Ready {
        Code::WindowsHostDetected
    } else {
        Code::WindowsHostUnsupported
    };
    let mut result = fact(
        state,
        code,
        Some(Observation::WindowsHost {
            product,
            edition,
            build_number,
            architecture,
        }),
    );
    if build_number == 0 {
        result.state = State::Failed;
        result.code = Code::WindowsProbeMalformed;
        result.observation = None;
    }
    result
}

pub(super) fn feature_fact(read: ProbeRead<OptionalFeatureState>) -> ProbeFact {
    let (state, code, observation) = match read {
        ProbeRead::Found(OptionalFeatureState::Enabled) => (
            State::Ready,
            Code::WindowsFeatureEnabled,
            Some(Observation::WindowsOptionalFeature {
                enabled: true,
                pending_reboot: false,
            }),
        ),
        ProbeRead::Found(OptionalFeatureState::Disabled) => (
            State::Missing,
            Code::WindowsFeatureDisabled,
            Some(Observation::WindowsOptionalFeature {
                enabled: false,
                pending_reboot: false,
            }),
        ),
        ProbeRead::Found(OptionalFeatureState::Pending) => (
            State::Missing,
            Code::WindowsFeaturePendingReboot,
            Some(Observation::WindowsOptionalFeature {
                enabled: false,
                pending_reboot: true,
            }),
        ),
        ProbeRead::Missing => (State::Missing, Code::WindowsFeatureDisabled, None),
        ProbeRead::Inaccessible => (State::Unknown, Code::WindowsProbeInaccessible, None),
        ProbeRead::Malformed => (State::Failed, Code::WindowsProbeMalformed, None),
    };
    fact(state, code, observation)
}

pub(super) fn kernel_fact(read: ProbeRead<KernelVersion>) -> ProbeFact {
    match read {
        ProbeRead::Found(version) if version.major > 0 => fact(
            State::Ready,
            Code::WindowsWslKernelDetected,
            Some(Observation::WindowsWslKernel {
                version: Some(version),
            }),
        ),
        ProbeRead::Found(_) | ProbeRead::Malformed => {
            fact(State::Failed, Code::WindowsProbeMalformed, None)
        }
        ProbeRead::Missing => fact(
            State::Missing,
            Code::WindowsWslKernelUnknown,
            Some(Observation::WindowsWslKernel { version: None }),
        ),
        ProbeRead::Inaccessible => fact(State::Unknown, Code::WindowsProbeInaccessible, None),
    }
}

pub(super) fn reboot_fact(read: ProbeRead<bool>) -> ProbeFact {
    match read {
        ProbeRead::Found(true) => fact(
            State::Missing,
            Code::WindowsRebootPending,
            Some(Observation::WindowsRebootPending { pending: true }),
        ),
        ProbeRead::Found(false) => fact(
            State::Ready,
            Code::WindowsNoRebootPending,
            Some(Observation::WindowsRebootPending { pending: false }),
        ),
        ProbeRead::Missing | ProbeRead::Inaccessible => {
            fact(State::Unknown, Code::WindowsProbeInaccessible, None)
        }
        ProbeRead::Malformed => fact(State::Failed, Code::WindowsProbeMalformed, None),
    }
}

pub(super) fn policy_fact(read: ProbeRead<Option<bool>>) -> ProbeFact {
    match read {
        ProbeRead::Found(Some(false)) => fact(
            State::Blocked,
            Code::WindowsWslPolicyBlocked,
            Some(Observation::WindowsWslPolicy {
                allowed: Some(false),
            }),
        ),
        ProbeRead::Found(Some(true)) => fact(
            State::Ready,
            Code::WindowsWslPolicyAllowed,
            Some(Observation::WindowsWslPolicy {
                allowed: Some(true),
            }),
        ),
        ProbeRead::Found(None) => fact(
            State::Ready,
            Code::WindowsWslPolicyAllowed,
            Some(Observation::WindowsWslPolicy { allowed: None }),
        ),
        ProbeRead::Missing => fact(
            State::Ready,
            Code::WindowsWslPolicyAllowed,
            Some(Observation::WindowsWslPolicy { allowed: None }),
        ),
        ProbeRead::Inaccessible => fact(State::Unknown, Code::WindowsProbeInaccessible, None),
        ProbeRead::Malformed => fact(State::Failed, Code::WindowsProbeMalformed, None),
    }
}

pub(super) fn edition_from_id(value: &str) -> Edition {
    let value = value.trim().to_ascii_lowercase();
    if value.starts_with("core") || value == "home" {
        Edition::Home
    } else if value.starts_with("professional") || value == "pro" {
        Edition::Pro
    } else if value.starts_with("enterprise") {
        Edition::Enterprise
    } else if value.starts_with("education") {
        Edition::Education
    } else if value.starts_with("server") {
        Edition::Server
    } else {
        Edition::Other
    }
}

pub(super) fn product_from_facts(edition: Edition, build_number: u32) -> Product {
    if edition == Edition::Server {
        Product::Server
    } else if build_number >= 22_000 {
        Product::Windows11
    } else if build_number >= 10_240 {
        Product::Windows10
    } else {
        Product::Unsupported
    }
}

pub(super) fn virtualization_fact(read: ProbeRead<bool>) -> ProbeFact {
    match read {
        ProbeRead::Found(true) => fact(
            State::Ready,
            Code::WindowsFeatureEnabled,
            Some(Observation::WindowsVirtualization { enabled: true }),
        ),
        ProbeRead::Found(false) => fact(
            State::Missing,
            Code::WindowsFeatureDisabled,
            Some(Observation::WindowsVirtualization { enabled: false }),
        ),
        ProbeRead::Missing | ProbeRead::Inaccessible => {
            fact(State::Unknown, Code::WindowsProbeInaccessible, None)
        }
        ProbeRead::Malformed => fact(State::Failed, Code::WindowsProbeMalformed, None),
    }
}
