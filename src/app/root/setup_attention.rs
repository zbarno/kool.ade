use super::{KooladeApp, Screen};
use std::time::Duration;

impl KooladeApp {
    fn clear_setup_attention_notices(&mut self) {
        self.setup_attention.clear();
        self.setup_attention_notified = None;
    }

    pub(super) fn queue_manager_setup_update(&mut self) {
        let active = self
            .machine_setup_tasks
            .iter()
            .filter(|task| task.is_active())
            .collect::<Vec<_>>();
        let notices = self
            .setup_attention
            .iter()
            .filter(|notice| !crate::app::setup_attention::has_specific_task(&active, notice))
            .collect::<Vec<_>>();
        if active.is_empty() && notices.is_empty() {
            self.setup_attention_notified = None;
            return;
        }

        let fingerprint = {
            use sha2::Digest;
            let mut entries = active
                .iter()
                .map(|task| format!("{}:{:?}", task.id, task.state))
                .chain(
                    notices
                        .iter()
                        .map(|notice| format!("{}:{}", notice.id, notice.issue)),
                )
                .collect::<Vec<_>>();
            entries.sort();
            format!("{:x}", sha2::Sha256::digest(entries.join("\n").as_bytes()))
        };
        if self.setup_attention_notified.as_deref() == Some(&fingerprint) {
            return;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        for task in active.iter().take(4) {
            project.activity.pending.push(format!(
                "Machine setup needs attention: {}. {} Next action: {}",
                task.title,
                crate::core::context_build::clip(&task.explanation, 400),
                task.next_step
            ));
        }
        for notice in notices.iter().take(4) {
            project.activity.pending.push(format!(
                "Workspace setup needs attention: {}. {} Next action: {}",
                notice.title,
                crate::core::context_build::clip(&notice.issue, 400),
                notice.next_action
            ));
        }
        self.setup_attention_notified = Some(fingerprint);
    }

    pub(super) fn refresh_setup_attention(&mut self, retry: bool) {
        self.clear_setup_attention_notices();
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
        result: anyhow::Result<crate::persistence::setup_tasks::Store>,
    ) -> Option<bool> {
        let Some(retry) = self.setup_refresh_pending.take() else {
            match result {
                Ok(store) => match crate::persistence::setup_tasks::ordered_task_ids(&store) {
                    Ok(ids) => {
                        self.machine_setup_tasks = ids
                            .iter()
                            .filter_map(|id| store.tasks.get(id).cloned())
                            .collect();
                        self.setup_attention.clear();
                    }
                    Err(error) => {
                        self.setup_attention =
                            vec![crate::app::setup_attention::SetupIssue::provider(format!(
                                "Could not safely order machine setup tasks: {error}"
                            ))];
                    }
                },
                Err(error) => {
                    self.setup_attention = vec![crate::app::setup_attention::SetupIssue::provider(
                        format!("Could not safely refresh machine setup tasks: {error}"),
                    )];
                }
            }
            return None;
        };
        self.setup_retry_requested = retry;
        Some(retry)
    }

    pub(super) fn poll_setup_attention(&mut self, ctx: &egui::Context) {
        let Some(handle) = self.setup_probe.take() else {
            return;
        };
        if handle.is_finished() {
            let result = handle.join().unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "The machine setup check stopped unexpectedly."
                ))
            });
            let succeeded = result.is_ok();
            if self.finish_setup_probe_result(result).is_some() {
                self.setup_probe = Some(std::thread::spawn(super::super::setup_attention::detect));
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                let active = self
                    .machine_setup_tasks
                    .iter()
                    .find(|task| task.is_active());
                if self.setup_retry_requested && succeeded && active.is_none() {
                    self.toasts
                        .success("Kool.ad/e setup is ready. Retry planning or implementation.");
                } else if self.setup_retry_requested
                    && let Some(notice) = self.setup_attention.first()
                {
                    self.toasts.warning(notice.next_action);
                } else if self.setup_retry_requested
                    && let Some(task) = active
                {
                    self.toasts.warning(task.next_step.clone());
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
    use crate::persistence::setup_tasks::{
        SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1, SetupTaskV1,
    };

    fn task() -> SetupTaskV1 {
        SetupTaskV1 {
            schema_version: 1,
            id: "setup:images:general:amd64".into(),
            title: "Prepare image".into(),
            explanation: "No approved image is verified.".into(),
            next_step: "Review image readiness.".into(),
            dependencies: Vec::new(),
            blocked_by: Vec::new(),
            state: SetupTaskStateV1::ReadyToFix,
            created_at_unix_ms: 1,
            observed_at_unix_ms: 1,
            scan_generation: 1,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: vec![SetupTaskEventV1 {
                observed_at_unix_ms: 1,
                scan_generation: 1,
                state: SetupTaskStateV1::ReadyToFix,
            }],
            legacy_source_id: None,
            superseded_by: Vec::new(),
        }
    }

    #[test]
    fn repeated_refresh_coalesces_retry_without_dropping_persisted_tasks() {
        let mut app = KooladeApp::default();
        app.machine_setup_tasks.push(task());
        app.setup_attention
            .push(crate::app::setup_attention::SetupIssue::provider(
                "Old transient setup notice",
            ));
        app.setup_probe = Some(std::thread::spawn(|| Ok(SetupTaskStoreV1::default())));

        app.refresh_setup_attention(false);
        assert_eq!(app.setup_refresh_pending, Some(false));
        assert!(app.setup_attention.is_empty());
        assert_eq!(app.machine_setup_tasks.len(), 1);
        app.refresh_setup_attention(true);
        app.refresh_setup_attention(false);
        assert_eq!(app.setup_refresh_pending, Some(true));

        let rerun = app.finish_setup_probe_result(Ok(SetupTaskStoreV1::default()));
        assert_eq!(rerun, Some(true));
        assert!(app.setup_attention.is_empty());
        assert_eq!(app.machine_setup_tasks.len(), 1);
        assert!(app.setup_refresh_pending.is_none());
        assert!(app.setup_retry_requested);
        app.setup_probe.take().unwrap().join().unwrap().unwrap();
    }

    #[test]
    fn completed_scan_replaces_the_projection_from_the_persisted_collection() {
        let mut app = KooladeApp::default();
        let mut store = SetupTaskStoreV1::default();
        store.tasks.insert(task().id.clone(), task());
        assert_eq!(app.finish_setup_probe_result(Ok(store)), None);
        assert_eq!(app.machine_setup_tasks.len(), 1);
        assert!(app.setup_attention.is_empty());
    }

    #[test]
    fn completed_scan_projects_machine_tasks_in_topological_order() {
        use crate::persistence::setup_tasks::{
            DependencyResolution, SetupDependencyV1, SetupTaskStateV1,
        };

        let mut app = KooladeApp::default();
        let parent_id = "setup:cli:install";
        let mut child = task();
        child.id = "setup:cli:codex:auth".into();
        child.state = SetupTaskStateV1::BlockedByPrerequisite;
        child.dependencies = vec![SetupDependencyV1 {
            prerequisite_id: parent_id.into(),
            resolution: DependencyResolution::Pending,
        }];
        child.blocked_by = vec![parent_id.into()];
        let mut parent = task();
        parent.id = parent_id.into();

        let mut store = SetupTaskStoreV1::default();
        store.tasks.insert(child.id.clone(), child);
        store.tasks.insert(parent.id.clone(), parent);
        assert_eq!(app.finish_setup_probe_result(Ok(store)), None);
        assert_eq!(
            app.machine_setup_tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["setup:cli:install", "setup:cli:codex:auth"]
        );
    }
}
