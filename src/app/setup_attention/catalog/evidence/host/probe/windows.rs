use crate::app::setup_attention::catalog::{EvidenceCode as Code, EvidenceState as State};

use super::{ProbeFact, fact};

#[cfg(target_os = "windows")]
mod system;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub(crate) struct WindowsFacts {
    pub(crate) host: ProbeFact,
    pub(crate) wsl_feature: ProbeFact,
    pub(crate) vm_platform_feature: ProbeFact,
    pub(crate) wsl_kernel: ProbeFact,
    pub(crate) reboot: ProbeFact,
    pub(crate) policy: ProbeFact,
    pub(crate) virtualization: ProbeFact,
}

impl WindowsFacts {
    pub(crate) fn not_applicable() -> Self {
        let na = || fact(State::NotApplicable, Code::HostCapabilityNotProbed, None);
        Self {
            host: na(),
            wsl_feature: na(),
            vm_platform_feature: na(),
            wsl_kernel: na(),
            reboot: na(),
            policy: na(),
            virtualization: na(),
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn unavailable() -> Self {
        let unknown = || fact(State::Unknown, Code::HostCapabilityNotProbed, None);
        Self {
            host: unknown(),
            wsl_feature: unknown(),
            vm_platform_feature: unknown(),
            wsl_kernel: unknown(),
            reboot: unknown(),
            policy: unknown(),
            virtualization: unknown(),
        }
    }

    pub(crate) fn wsl2_state(&self) -> State {
        if self.policy.state == State::Blocked {
            return State::Blocked;
        }
        if self.host.state == State::Unsupported {
            return State::Unsupported;
        }
        if self.virtualization.state == State::Missing {
            return State::Blocked;
        }
        if self.reboot.state == State::Missing
            || self.wsl_feature.state == State::Missing
            || self.vm_platform_feature.state == State::Missing
            || self.wsl_kernel.state == State::Missing
        {
            return State::Missing;
        }
        if self.host.state != State::Ready
            || self.virtualization.state != State::Ready
            || self.policy.state != State::Ready
            || self.reboot.state != State::Ready
            || self.wsl_feature.state != State::Ready
            || self.vm_platform_feature.state != State::Ready
            || self.wsl_kernel.state != State::Ready
        {
            return State::Unknown;
        }
        State::Ready
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn collect() -> WindowsFacts {
    system::collect()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect() -> WindowsFacts {
    WindowsFacts::unavailable()
}

mod classify;
mod parsers;
