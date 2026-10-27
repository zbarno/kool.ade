use crate::execution::docker::images::RuntimeProfile;

use super::SetupTaskStoreV1;

pub(super) const MAX_REQUIRED_IMAGE_PROFILES: usize = 5;

pub(super) fn supported_profile(profile: RuntimeProfile) -> bool {
    matches!(
        profile,
        RuntimeProfile::General
            | RuntimeProfile::Rust
            | RuntimeProfile::Node
            | RuntimeProfile::Python
            | RuntimeProfile::Go
    )
}

impl SetupTaskStoreV1 {
    pub(in crate::persistence::setup_tasks) fn require_image_profile(
        &mut self,
        profile: RuntimeProfile,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            supported_profile(profile),
            "runtime QA image profiles cannot be requested for bootstrap"
        );
        self.required_image_profiles.insert(profile);
        anyhow::ensure!(
            self.required_image_profiles.len() <= MAX_REQUIRED_IMAGE_PROFILES,
            "too many required runtime image profiles"
        );
        Ok(())
    }
}
