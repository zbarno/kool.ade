use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ImageArchitectureV1 as Architecture,
    ProbeEvidenceV1, ProbeObservationV1 as Observation, WindowsEditionV1 as Edition,
    WindowsProductV1 as Product,
};

pub(crate) fn observation_is_consistent(item: &ProbeEvidenceV1) -> bool {
    match item.observation.as_ref() {
        Some(Observation::WindowsHost {
            product,
            edition,
            build_number,
            architecture,
        }) => {
            if *build_number == 0 || *architecture == Architecture::Unknown {
                return false;
            }
            let supported = !matches!(*edition, Edition::Other | Edition::Server)
                && *architecture == Architecture::Amd64
                && *product == Product::Windows11
                && *build_number >= 22_000;
            let (state, code) = if *edition == Edition::Other {
                (State::Unknown, Code::WindowsHostUnsupported)
            } else if supported {
                (State::Ready, Code::WindowsHostDetected)
            } else {
                (State::Unsupported, Code::WindowsHostUnsupported)
            };
            item.state == state && item.code == code
        }
        Some(Observation::WindowsOptionalFeature {
            enabled,
            pending_reboot,
        }) => matches!(
            (*enabled, *pending_reboot, item.state, item.code),
            (true, false, State::Ready, Code::WindowsFeatureEnabled)
                | (false, false, State::Missing, Code::WindowsFeatureDisabled)
                | (
                    false,
                    true,
                    State::Missing,
                    Code::WindowsFeaturePendingReboot
                )
        ),
        Some(Observation::WindowsWslKernel { version }) => match (version, item.state, item.code) {
            (Some(version), State::Ready, Code::WindowsWslKernelDetected) => {
                version.major >= 4 && version.patch < 10_000
            }
            (None, State::Missing, Code::WindowsWslKernelUnknown) => true,
            _ => false,
        },
        Some(Observation::WindowsRebootPending { pending }) => matches!(
            (*pending, item.state, item.code),
            (true, State::Missing, Code::WindowsRebootPending)
                | (false, State::Ready, Code::WindowsNoRebootPending)
        ),
        Some(Observation::WindowsWslPolicy { allowed }) => matches!(
            (*allowed, item.state, item.code),
            (Some(false), State::Blocked, Code::WindowsWslPolicyBlocked)
                | (Some(true), State::Ready, Code::WindowsWslPolicyAllowed)
                | (None, State::Ready, Code::WindowsWslPolicyAllowed)
        ),
        Some(Observation::WindowsVirtualization { enabled }) => matches!(
            (*enabled, item.state, item.code),
            (true, State::Ready, Code::WindowsFeatureEnabled)
                | (false, State::Missing, Code::WindowsFeatureDisabled)
        ),
        _ => true,
    }
}
