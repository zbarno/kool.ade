use super::KooladeApp;
use crate::persistence::setup_tasks::{LocalTask, State};

impl KooladeApp {
    pub(super) fn publish_setup_task_store_update(&mut self) {
        let mut published = false;
        if let Some(store) = self.setup_image_preparation.take_store_update() {
            let current_generation = self
                .machine_setup_tasks
                .iter()
                .map(|task| task.scan_generation)
                .max()
                .unwrap_or(0);
            if store.latest_scan_generation >= current_generation {
                self.machine_setup_tasks = store.tasks.into_values().collect();
                published = true;
            }
        }
        self.setup_image_preparation
            .acknowledge_authorization_published(published, &self.machine_setup_tasks);
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
