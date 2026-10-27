use std::collections::BTreeSet;

use crate::execution::docker::images::RuntimeProfile;

use super::{EvidenceCode, EvidenceState, GitProbeV1};

#[derive(Clone, Debug, Default)]
pub(crate) struct ImageScanState {
    pub(crate) required_profiles: BTreeSet<RuntimeProfile>,
    pub(crate) smoke_verified_profiles: BTreeSet<RuntimeProfile>,
}

#[derive(Clone, Debug)]
pub(crate) struct ScanInputs {
    pub(crate) git_probe: GitProbeV1,
    pub(crate) images: ImageScanState,
}

impl Default for ScanInputs {
    fn default() -> Self {
        Self {
            git_probe: GitProbeV1 {
                executable_state: EvidenceState::Unknown,
                executable_code: EvidenceCode::GitProbeNotRun,
                identity_state: EvidenceState::Blocked,
                identity_code: EvidenceCode::ParentPrerequisiteUnavailable,
            },
            images: ImageScanState::default(),
        }
    }
}
