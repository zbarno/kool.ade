use super::PacketApp;
use std::time::Duration;

impl PacketApp {
    pub(super) fn refresh_setup_attention(&mut self, retry: bool) {
        if self.setup_probe.is_some() {
            return;
        }
        if let Some(issue) = super::super::setup_attention::detect_sandbox() {
            self.setup_attention = Some(issue);
            self.setup_retry_requested = false;
            return;
        }
        self.setup_retry_requested = retry;
        self.setup_probe = Some(std::thread::spawn(
            super::super::setup_attention::detect_provider,
        ));
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
            if self.setup_retry_requested && self.setup_attention.is_none() {
                self.toasts
                    .success("Packet setup is ready. Retry planning or implementation.");
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
