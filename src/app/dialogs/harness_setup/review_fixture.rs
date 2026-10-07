use super::*;

impl DlgHarnessSetup {
    pub(crate) fn review_fixture(
        settings: crate::persistence::harness_settings::HarnessSettings,
    ) -> Self {
        Self {
            settings,
            probe_view: ProbeView::Complete,
            feedback: None,
            section: HarnessSettingsSection::Tools,
            manual_path_drafts: Default::default(),
            probe_rx: None,
        }
    }
}
