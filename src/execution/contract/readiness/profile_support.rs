use super::{
    AuthenticationModeV1, HarnessCapabilitySnapshotV1, current_authentication_mode,
    profile_evidence_id_for_snapshot,
};

impl HarnessCapabilitySnapshotV1 {
    pub fn authentication_mode_is_current(&self) -> bool {
        self.authentication_mode != AuthenticationModeV1::Unknown
            && self.authentication_mode == current_authentication_mode(&self.provider_id)
    }

    pub fn profile_evidence_is_current(&self) -> bool {
        self.authentication_mode_is_current()
            && self.manager_profile_id
                == profile_evidence_id_for_snapshot(
                    &self.provider_id,
                    self.cli_version.as_deref(),
                    self.authentication_mode,
                    crate::harness::ExecutionProfile::Manager,
                )
            && self.worker_profile_id
                == profile_evidence_id_for_snapshot(
                    &self.provider_id,
                    self.cli_version.as_deref(),
                    self.authentication_mode,
                    crate::harness::ExecutionProfile::Worker,
                )
    }

    pub fn supports_profile(&self, profile: crate::harness::ExecutionProfile) -> bool {
        if self.validate().is_err() || !self.authentication_mode_is_current() {
            return false;
        }
        let expected = profile_evidence_id_for_snapshot(
            &self.provider_id,
            self.cli_version.as_deref(),
            self.authentication_mode,
            profile,
        );
        !expected.is_empty()
            && match profile {
                crate::harness::ExecutionProfile::Manager => self.manager_profile_id == expected,
                crate::harness::ExecutionProfile::Worker => self.worker_profile_id == expected,
            }
    }
}
