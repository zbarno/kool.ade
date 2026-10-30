//! Explicit combined choice; selecting setup priority alone keeps its old semantics.
use super::DlgHarnessSetup;
use crate::persistence::harness_settings::{self, MANAGER, WorkRoute};

impl DlgHarnessSetup {
    pub(super) fn onboarding_model(&self, id: &str) -> Option<String> {
        if !super::supported_provider_id(id) {
            return None;
        }
        let now = harness_settings::current_unix_ms();
        let harness = self.settings.discovered.get(id)?;
        let role = crate::app::root::role_for_work_category(MANAGER)?;
        if !harness.ready
            || crate::harness::execution_security::task_execution_block_reason(id).is_some()
            || !harness.has_current_authenticated_capability(id, now)
            || !harness
                .capability_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.supports_context_only_help())
            || !harness.supports_role_at(role, now)
            || harness.configuration_required
        {
            return None;
        }
        harness
            .verified_default_model_at(id, now)
            .map(str::to_owned)
    }

    pub(super) fn select_onboarding_manager(&mut self, id: &str) {
        let Some(model) = self.onboarding_model(id) else {
            self.set_feedback(
                false,
                "Verify sign-in, a default model, and planning capability first.".into(),
            );
            return;
        };
        let previous = self.settings.clone();
        self.settings.work_routes.insert(
            MANAGER.into(),
            WorkRoute {
                harness: id.into(),
                model: Some(model),
            },
        );
        self.settings.bootstrap_provider = Some(id.into());
        if !self.persist_saved() {
            self.settings = previous;
        }
    }
}

#[cfg(test)]
mod tests;
