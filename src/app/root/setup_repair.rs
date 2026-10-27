use super::KooladeApp;
use crate::persistence::setup_tasks::{LocalTask, State};

impl KooladeApp {
    pub(super) fn publish_setup_task_store_update(&mut self) {
        let mut published = false;
        let store = self
            .setup_image_preparation
            .take_store_update()
            .or_else(|| self.setup_remediation.take_store_update());
        if let Some(store) = store {
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
        self.setup_remediation
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
        if !can_prepare(task) {
            self.toasts.warning(if task.blocked_by.is_empty() {
                "A guided repair is not available for this setup task yet."
            } else {
                "Complete the listed prerequisite before requesting this repair."
            });
            return;
        }
        if self.setup_image_preparation.is_busy() || self.setup_remediation.is_busy() {
            self.toasts
                .warning("Finish the current machine repair before starting another one.");
            return;
        }
        if supported_image_task(task) {
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
            .is_some_and(can_approve);
        if !still_ready {
            if is_rootless_task(task_id) {
                self.setup_remediation.cancel_review();
            } else {
                self.setup_image_preparation.cancel_review();
            }
            self.toasts.warning(
                "This setup task changed while the repair was under review. Check again before approving it.",
            );
            return;
        }
        if is_rootless_task(task_id) {
            let Some(request) = self.setup_remediation.take_approval(task_id) else {
                self.toasts
                    .warning("The repair approval expired. Review it again before continuing.");
                self.setup_remediation.cancel_review();
                return;
            };
            self.setup_remediation.approve(request);
        } else {
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

fn can_prepare(task: &LocalTask) -> bool {
    (supported_image_task(task) || is_rootless_task(&task.id))
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

fn can_approve(task: &LocalTask) -> bool {
    (supported_image_task(task) || is_rootless_task(&task.id))
        && task.blocked_by.is_empty()
        && task.state == State::NeedsApproval
}

fn supported_image_task(task: &LocalTask) -> bool {
    crate::app::bootstrap_repair_policy::parse_locked_image_task_id(&task.id).is_some()
}

fn is_rootless_task(task_id: &str) -> bool {
    matches!(
        task_id,
        "setup:linux:rootless:packages"
            | "setup:linux:rootless:subids"
            | "setup:linux:rootless:service"
    )
}
