use super::KooladeApp;
use crate::app::setup_workflow::{FixRoute, can_decide_fix, can_request_fix, fix_route};

impl KooladeApp {
    pub(super) fn publish_setup_task_store_update(&mut self) {
        let mut published = false;
        if let Some(store) = self.setup_image_preparation.take_store_update() {
            published |= self.apply_setup_task_store(store);
        }
        if let Some(store) = self.claude_install.take_store_update() {
            published |= self.apply_setup_task_store(store);
        }
        if let Some(store) = self.setup_remediation.take_store_update() {
            published |= self.apply_setup_task_store(store);
        }
        self.setup_image_preparation
            .acknowledge_authorization_published(published, &self.machine_setup_tasks);
        self.setup_remediation
            .acknowledge_authorization_published(published, &self.machine_setup_tasks);
    }

    fn apply_setup_task_store(
        &mut self,
        store: crate::persistence::setup_tasks::SetupTaskStoreV1,
    ) -> bool {
        let store = crate::persistence::setup_tasks::load().unwrap_or(store);
        let current_generation = self
            .machine_setup_tasks
            .iter()
            .map(|task| task.scan_generation)
            .max()
            .unwrap_or(0);
        if store.latest_scan_generation < current_generation {
            return false;
        }
        self.machine_setup_tasks = store.tasks.into_values().collect();
        true
    }

    pub(super) fn request_setup_task_fix(&mut self, task_id: &str) {
        let Some(task) = self
            .machine_setup_tasks
            .iter()
            .find(|task| task.id == task_id)
        else {
            self.toasts
                .warning("This setup task is no longer available. Check the machine again.");
            return;
        };
        let route = fix_route(task);
        if route == Some(FixRoute::ClaudeInstall) {
            if !can_request_fix(task)
                || !crate::app::bootstrap_remediation::ClaudeInstallController::supports_current_host()
            {
                self.toasts.warning(
                    "This Claude install recipe is unavailable for the current machine state.",
                );
                return;
            }
            if self.setup_image_preparation.is_busy()
                || self.setup_remediation.is_busy()
                || claude_install_busy(self)
            {
                self.toasts
                    .warning("Finish the current machine repair before starting another one.");
                return;
            }
            match self.claude_install.request_review(task_id) {
                Ok(()) => self.publish_setup_task_store_update(),
                Err(error) => self.toasts.warning(format!(
                    "Could not prepare the Claude Code install review: {error}"
                )),
            }
            return;
        }
        if !can_request_fix(task) {
            self.toasts.warning(if task.blocked_by.is_empty() {
                "A guided repair is not available for this setup task yet."
            } else {
                "Complete the listed prerequisite before requesting this repair."
            });
            return;
        }
        if self.setup_image_preparation.is_busy()
            || self.setup_remediation.is_busy()
            || claude_install_busy(self)
        {
            self.toasts
                .warning("Finish the current machine repair before starting another one.");
            return;
        }
        if route == Some(FixRoute::Image) {
            self.setup_image_preparation.request_review(task_id);
        } else {
            self.setup_remediation.request_review(task_id);
        }
    }

    pub(super) fn approve_setup_task_fix(&mut self, task_id: &str) {
        let still_ready = self
            .machine_setup_tasks
            .iter()
            .find(|task| task.id == task_id)
            .is_some_and(can_decide_fix);
        let route = self
            .machine_setup_tasks
            .iter()
            .find(|task| task.id == task_id)
            .and_then(fix_route);
        if !still_ready {
            match route {
                Some(FixRoute::Remediation) => self.setup_remediation.cancel_review(),
                Some(FixRoute::ClaudeInstall) => {
                    let _ = self.claude_install.cancel_review();
                    self.publish_setup_task_store_update();
                }
                _ => self.setup_image_preparation.cancel_review(),
            }
            self.toasts.warning(
                "This setup task changed while the repair was under review. Check again before approving it.",
            );
            return;
        }
        match route {
            Some(FixRoute::Remediation) => {
                let Some(request) = self.setup_remediation.take_approval(task_id) else {
                    self.toasts
                        .warning("The repair approval expired. Review it again before continuing.");
                    self.setup_remediation.cancel_review();
                    return;
                };
                self.setup_remediation.approve(request);
            }
            Some(FixRoute::ClaudeInstall) => {
                // The staged approval request can only be raised through the
                // dedicated Install Claude Code review, which the consent
                // panel points at. Fabricating it here would authorize an
                // unstaged operation, and cancelling would hit an unrelated
                // controller's review.
                self.toasts
                    .info("Approve this install in the Install Claude Code review on this screen.");
            }
            Some(FixRoute::Image) | None => {
                let Some(request) = self.setup_image_preparation.take_approval(task_id) else {
                    self.toasts
                        .warning("The repair approval expired. Review it again before continuing.");
                    self.setup_image_preparation.cancel_review();
                    return;
                };
                self.setup_image_preparation.approve(request);
            }
        }
    }
}

fn claude_install_busy(app: &KooladeApp) -> bool {
    matches!(
        app.claude_install.status(),
        crate::app::bootstrap_remediation::ClaudeInstallStatus::NeedsApproval
            | crate::app::bootstrap_remediation::ClaudeInstallStatus::Running
    )
}
