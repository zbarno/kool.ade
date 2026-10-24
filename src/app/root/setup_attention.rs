use super::KooladeApp;
use std::time::Duration;

impl KooladeApp {
    pub(super) fn queue_manager_setup_update(&mut self) {
        let Some(issue) = self.setup_attention.clone() else {
            self.setup_attention_notified = None;
            return;
        };
        let fingerprint = format!("{}:{}", issue.id, issue.issue);
        if self.setup_attention_notified.as_deref() == Some(&fingerprint) {
            return;
        }
        let super::Screen::Connected(project) = &mut self.screen else {
            return;
        };
        project.activity.pending.push(format!(
            "Workspace setup needs attention: {}. {} Next action: {}",
            issue.title,
            crate::core::context_build::clip(&issue.issue, 400),
            issue.next_action
        ));
        self.setup_attention_notified = Some(fingerprint);
    }

    pub(super) fn refresh_setup_attention(&mut self, retry: bool) {
        if self.setup_probe.is_some() {
            return;
        }
        self.setup_retry_requested = retry;
        self.setup_probe = Some(std::thread::spawn(super::super::setup_attention::detect));
    }

    pub(super) fn poll_setup_attention(&mut self, ctx: &egui::Context) {
        let Some(handle) = self.setup_probe.take() else {
            return;
        };
        if handle.is_finished() {
            self.setup_attention = match handle.join() {
                Ok(issue) => issue,
                Err(_) => Some(super::super::setup_attention::SetupIssue::provider(
                    "The provider setup check stopped unexpectedly. Retry the check.",
                )),
            };
            // Scan writes machine-local tasks outside all shared planning
            // repositories. Only reload after the scan worker completed.
            match crate::persistence::setup_tasks::load() {
                Ok(store) => {
                    self.machine_setup_tasks = store.tasks.into_values().collect();
                }
                Err(error) => {
                    self.toasts
                        .warning(format!("Could not load local setup tasks: {error}"));
                }
            }
            if self.setup_retry_requested && self.setup_attention.is_none() {
                self.toasts
                    .success("Kool.ad/e setup is ready. Retry planning or implementation.");
            } else if self.setup_retry_requested
                && let Some(issue) = &self.setup_attention
            {
                self.toasts.warning(issue.next_action);
            }
            self.setup_retry_requested = false;
        } else {
            self.setup_probe = Some(handle);
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}
