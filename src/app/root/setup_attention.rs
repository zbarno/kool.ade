use super::KooladeApp;
use std::time::Duration;

impl KooladeApp {
    fn clear_setup_attention_snapshot(&mut self) {
        self.setup_attention = None;
        self.machine_setup_tasks.clear();
        self.setup_attention_notified = None;
    }

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
        self.clear_setup_attention_snapshot();
        if self.setup_probe.is_some() {
            self.setup_refresh_pending = Some(self.setup_refresh_pending.unwrap_or(false) || retry);
            return;
        }
        self.setup_refresh_pending = None;
        self.setup_retry_requested = retry;
        self.setup_probe = Some(std::thread::spawn(super::super::setup_attention::detect));
    }

    fn finish_setup_probe_result(
        &mut self,
        result: Option<super::super::setup_attention::SetupIssue>,
    ) -> Option<bool> {
        let Some(retry) = self.setup_refresh_pending.take() else {
            self.setup_attention = result;
            return None;
        };
        self.clear_setup_attention_snapshot();
        self.setup_retry_requested = retry;
        Some(retry)
    }

    pub(super) fn poll_setup_attention(&mut self, ctx: &egui::Context) {
        let Some(handle) = self.setup_probe.take() else {
            return;
        };
        if handle.is_finished() {
            let result = match handle.join() {
                Ok(issue) => issue,
                Err(_) => Some(super::super::setup_attention::SetupIssue::provider(
                    "The provider setup check stopped unexpectedly. Retry the check.",
                )),
            };
            if self.finish_setup_probe_result(result).is_some() {
                self.setup_probe = Some(std::thread::spawn(super::super::setup_attention::detect));
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                // Scan writes machine-local tasks outside all shared planning
                // repositories. Reload only the completed scan's state.
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
            }
        } else {
            self.setup_probe = Some(handle);
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::KooladeApp;

    #[test]
    fn refresh_during_probe_is_coalesced_and_retry_intent_is_preserved() {
        let mut app = KooladeApp::default();
        app.setup_attention = Some(crate::app::setup_attention::SetupIssue::provider(
            "Stale machine readiness result",
        ));
        app.machine_setup_tasks
            .push(crate::persistence::setup_tasks::LocalTask {
                id: "setup:images:general:amd64".into(),
                title: "Prepare image".into(),
                explanation: "A stale task projection".into(),
                next_step: "Rescan".into(),
                prerequisites: Vec::new(),
                state: crate::persistence::setup_tasks::State::NeedsAttention,
                observed_at_unix_ms: 1,
                verified_at_unix_ms: None,
                events: Vec::new(),
            });
        app.setup_attention_notified = Some("stale:machine-scan".into());
        app.setup_probe = Some(std::thread::spawn(|| None));

        app.refresh_setup_attention(false);
        assert_eq!(app.setup_refresh_pending, Some(false));
        assert!(app.setup_attention.is_none());
        assert!(app.machine_setup_tasks.is_empty());
        assert!(app.setup_attention_notified.is_none());
        app.refresh_setup_attention(true);
        app.refresh_setup_attention(false);
        assert_eq!(app.setup_refresh_pending, Some(true));

        let rerun = app.finish_setup_probe_result(Some(
            crate::app::setup_attention::SetupIssue::provider("Stale probe result"),
        ));
        assert_eq!(rerun, Some(true));
        assert!(app.setup_attention.is_none());
        assert!(app.machine_setup_tasks.is_empty());
        assert!(app.setup_refresh_pending.is_none());
        assert!(app.setup_retry_requested);

        app.setup_probe.take().unwrap().join().unwrap();
    }
}
