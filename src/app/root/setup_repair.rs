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
        if is_claude_install_task(task) {
            if !can_prepare_claude_install(task)
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
        if !can_prepare(task) {
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
            if is_remediation_task(task_id) {
                self.setup_remediation.cancel_review();
            } else if task_id == "setup:cli:claude:install" {
                let _ = self.claude_install.cancel_review();
                self.publish_setup_task_store_update();
            } else {
                self.setup_image_preparation.cancel_review();
            }
            self.toasts.warning(
                "This setup task changed while the repair was under review. Check again before approving it.",
            );
            return;
        }
        if is_remediation_task(task_id) {
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

fn claude_install_busy(app: &KooladeApp) -> bool {
    matches!(
        app.claude_install.status(),
        crate::app::bootstrap_remediation::ClaudeInstallStatus::NeedsApproval
            | crate::app::bootstrap_remediation::ClaudeInstallStatus::Running
    )
}

fn can_prepare(task: &LocalTask) -> bool {
    (supported_image_task(task) || is_remediation_task(&task.id))
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
    (supported_image_task(task) || is_remediation_task(&task.id))
        && task.blocked_by.is_empty()
        && task.state == State::NeedsApproval
}

fn supported_image_task(task: &LocalTask) -> bool {
    crate::app::bootstrap_repair_policy::parse_locked_image_task_id(&task.id).is_some()
}

fn is_remediation_task(task_id: &str) -> bool {
    matches!(
        task_id,
        "setup:linux:rootless:packages"
            | "setup:linux:rootless:subids"
            | "setup:linux:rootless:service"
            | "setup:computer:qemu"
    ) || windows_setup_task_enabled(task_id)
}

fn windows_setup_task_enabled(task_id: &str) -> bool {
    cfg!(windows)
        && task_id == "setup:windows:wsl-features"
        && crate::windows_setup_helper::publisher_trust_configured()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wsl_features_task(state: State) -> LocalTask {
        LocalTask {
            schema_version: 1,
            id: "setup:windows:wsl-features".into(),
            title: "Enable WSL2 Windows features".into(),
            explanation: String::new(),
            next_step: String::new(),
            dependencies: Vec::new(),
            blocked_by: Vec::new(),
            state,
            created_at_unix_ms: 1,
            observed_at_unix_ms: 1,
            scan_generation: 1,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: Vec::new(),
            superseded_by: Vec::new(),
        }
    }

    #[test]
    fn windows_feature_task_requires_a_signed_windows_build_for_live_routing() {
        let ready = wsl_features_task(State::ReadyToFix);
        let awaiting_approval = wsl_features_task(State::NeedsApproval);
        let enabled = cfg!(windows) && crate::windows_setup_helper::publisher_trust_configured();
        assert_eq!(can_prepare(&ready), enabled);
        assert_eq!(can_approve(&awaiting_approval), enabled);
        assert_eq!(is_remediation_task(&ready.id), enabled);
    }

    #[test]
    fn qemu_task_uses_the_native_remediation_review_and_approval_gate() {
        let ready = LocalTask {
            id: "setup:computer:qemu".into(),
            ..wsl_features_task(State::ReadyToFix)
        };
        let awaiting_approval = LocalTask {
            state: State::NeedsApproval,
            ..ready.clone()
        };
        assert!(is_remediation_task(&ready.id));
        assert!(can_prepare(&ready));
        assert!(can_approve(&awaiting_approval));

        let blocked = LocalTask {
            blocked_by: vec!["setup:computer:kvm".into()],
            ..ready.clone()
        };
        assert!(!can_prepare(&blocked));
        assert!(!can_approve(&LocalTask {
            state: State::NeedsVerification,
            ..awaiting_approval
        }));
    }
}
