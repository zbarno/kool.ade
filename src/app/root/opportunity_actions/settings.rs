use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(crate) fn save_opportunity_discovery_settings(
        &mut self,
        settings: crate::core::opportunity::OpportunityDiscoverySettingsV1,
        expected_revision: u64,
    ) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let store = crate::core::opportunity::OpportunityStore::new(&project.state.planning_store);
        match store.save_discovery_settings(&settings, expected_revision) {
            Ok(saved) => self.toasts.success(format!(
                "Opportunity discovery settings saved at revision {}. Scheduled exploration is {}.",
                saved.revision,
                match saved.exploration {
                    crate::core::opportunity::OpportunityExploreModeV1::OptedIn => "enabled",
                    crate::core::opportunity::OpportunityExploreModeV1::Disabled => "disabled",
                }
            )),
            Err(error) => self.toasts.warning(format!(
                "Opportunity discovery settings were not saved: {error}. Refresh the board before trying again."
            )),
        }
    }
}
