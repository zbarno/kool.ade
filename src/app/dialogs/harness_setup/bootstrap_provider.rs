use super::DlgHarnessSetup;
use crate::persistence::harness_settings::HarnessSettings;

pub(super) fn set_provider(settings: &mut HarnessSettings, id: &str) -> bool {
    if !super::supported_provider_id(id) {
        return false;
    }
    settings.bootstrap_provider = Some(id.to_owned());
    true
}

impl DlgHarnessSetup {
    pub fn select_bootstrap_provider(&mut self, id: &str) {
        let previous = self.settings.clone();
        if !set_provider(&mut self.settings, id) {
            self.feedback = Some((false, format!("Unsupported provider: {id}")));
            return;
        }
        if self.persist_saved() {
            self.refresh();
        } else {
            self.settings = previous;
        }
    }

    pub fn clear_bootstrap_provider(&mut self) {
        if self.settings.bootstrap_provider.is_some() {
            let previous = self.settings.clone();
            self.settings.bootstrap_provider = None;
            if self.persist_saved() {
                self.refresh();
            } else {
                self.settings = previous;
            }
        }
    }
}

#[cfg(test)]
mod tests;
