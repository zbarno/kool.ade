use super::KooladeApp;
use crate::persistence::setup_tasks::{LocalTask, State};

impl KooladeApp {
    pub(super) fn publish_setup_task_store_update(&mut self) {
        let mut published = false;
        if let Some(store) = self.setup_image_preparation.take_store_update() {
            published |= self.apply_setup_task_store(store);
        }
        if let Some(store) = self.claude_install.take_store_update() {
            published |= self.apply_setup_task_store(store);
        }
        self.setup_image_preparation
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
        if is_claude_install_task(task) {
            if !can_prepare_claude_install(task)
                || !crate::app::bootstrap_remediation::ClaudeInstallController::supports_current_host()
            {
                self.toasts.warning(
                    "This Claude install recipe is unavailable for the current machine state.",
                );
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
        if !can_prepare_image(task) {
            self.toasts.warning(if task.blocked_by.is_empty() {
                "A guided repair is not available for this setup task yet."
            } else {
                "Complete the listed prerequisite before requesting this repair."
            });
            return;
        }
        self.setup_image_preparation.request_review(task_id);
    }

    pub(super) fn approve_setup_task_fix(&mut self, task_id: &str) {
        let still_ready = self
            .machine_setup_tasks
            .iter()
            .find(|task| task.id == task_id)
            .is_some_and(can_approve_image_fix);
        if !still_ready {
            self.setup_image_preparation.cancel_review();
            self.toasts.warning(
                "This setup task changed while the repair was under review. Check again before approving it.",
            );
            return;
        }
        let Some(request) = self.setup_image_preparation.take_approval(task_id) else {
            self.toasts
                .warning("The repair approval expired. Review it again before continuing.");
            self.setup_image_preparation.cancel_review();
            return;
        };
        self.setup_image_preparation.approve(request);
    }
}

fn is_claude_install_task(task: &LocalTask) -> bool {
    task.id == "setup:cli:claude:install"
}

fn can_prepare_claude_install(task: &LocalTask) -> bool {
    is_claude_install_task(task)
        && task.blocked_by.is_empty()
        && matches!(
            task.state,
            State::ReadyToFix
                | State::Failed
                | State::Paused
                | State::Interrupted
                | State::Cancelled
        )
}

fn can_prepare_image(task: &LocalTask) -> bool {
    supported_image_task(task)
        && task.blocked_by.is_empty()
        && matches!(
            task.state,
            State::ReadyToFix
                | State::Failed
                | State::Paused
                | State::Interrupted
                | State::Cancelled
        )
}

fn can_approve_image_fix(task: &LocalTask) -> bool {
    supported_image_task(task) && task.blocked_by.is_empty() && task.state == State::NeedsApproval
}

fn supported_image_task(task: &LocalTask) -> bool {
    task.id
        .strip_prefix("setup:images:general:")
        .is_some_and(|architecture| matches!(architecture, "amd64" | "arm64"))
}
