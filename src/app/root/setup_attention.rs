use super::{KooladeApp, Screen};
use std::time::Duration;

pub(super) enum ProbeOutput {
    Scanned(super::super::setup_attention::SetupScanResult),
    Verified(super::super::setup_attention::SetupScanVerification),
}

impl KooladeApp {
    fn clear_setup_attention_notices(&mut self) {
        self.setup_attention.clear();
        self.setup_attention_notified = None;
    }

    fn start_setup_probe(&mut self, recover_after_restart: bool) {
        let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancellation = cancellation.clone();
        self.setup_probe_cancel = Some(cancellation);
        #[cfg(test)]
        let state_path = Some(self.setup_task_state_path.clone());
        #[cfg(not(test))]
        let state_path = None;
        self.setup_probe = Some(std::thread::spawn(move || {
            ProbeOutput::Scanned(super::super::setup_attention::run_scan(
                recover_after_restart,
                worker_cancellation,
                state_path,
            ))
        }));
    }

    fn start_setup_identity_verification(
        &mut self,
        result: super::super::setup_attention::SetupScanResult,
    ) {
        let cancellation = self
            .setup_probe_cancel
            .get_or_insert_with(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
            .clone();
        let commit_lock = self.setup_commit_lock.clone();
        #[cfg(test)]
        let state_path = self.setup_task_state_path.clone();
        self.setup_probe = Some(std::thread::spawn(move || {
            #[cfg(test)]
            let verification = super::super::setup_attention::verify_before_commit_with_state_path(
                result,
                cancellation,
                commit_lock,
                state_path,
            );
            #[cfg(not(test))]
            let verification = super::super::setup_attention::verify_before_commit(
                result,
                cancellation,
                commit_lock,
            );
            ProbeOutput::Verified(verification)
        }));
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
            .filter(|notice| !super::super::setup_attention::has_specific_task(&active, notice))
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
        let setup_commit_lock = self.setup_commit_lock.clone();
        // A final identity check and local commit must not be interrupted midway.
        let commit_guard = match setup_commit_lock.try_lock() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(error)) => Some(error.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => None,
        };
        self.clear_setup_attention_notices();
        if commit_guard.is_some()
            && let Some(cancellation) = &self.setup_probe_cancel
        {
            cancellation.store(true, std::sync::atomic::Ordering::Release);
        }
        drop(commit_guard);
        if self.setup_probe.is_some() {
            self.setup_refresh_pending = Some(self.setup_refresh_pending.unwrap_or(false) || retry);
            return;
        }
        self.setup_refresh_pending = None;
        self.setup_retry_requested = retry;
        self.start_setup_probe(false);
    }

    fn finish_setup_probe_result(
        &mut self,
        verification: super::super::setup_attention::SetupScanVerification,
    ) -> Option<bool> {
        let result = verification.result;
        self.setup_probe_cancel = None;
        let retry = self.setup_refresh_pending.take();
        if result.cancelled || retry.is_some() {
            self.clear_setup_attention_notices();
            if let Some(retry) = retry {
                self.setup_retry_requested = retry;
                return Some(retry);
            }
            self.setup_retry_requested = false;
            return None;
        }
        if result.snapshot.is_none() {
            self.setup_attention = vec![result.issue.unwrap_or_else(|| {
                super::super::setup_attention::SetupIssue::provider(
                    "The machine setup check did not finish. Retry the check.",
                )
            })];
            return None;
        }

        self.setup_attention = result.issue.into_iter().collect();
        if verification.persistence_failed {
            self.setup_attention = vec![super::super::setup_attention::SetupIssue::provider(
                "Could not safely save local machine setup diagnostics. Retry the check.",
            )];
        } else if let Some(tasks) = verification.machine_setup_tasks {
            self.machine_setup_tasks = tasks;
        } else {
            self.setup_attention = vec![super::super::setup_attention::SetupIssue::provider(
                "The machine setup result was not persisted. Retry the check.",
            )];
        }
        None
    }

    pub(super) fn poll_setup_attention(&mut self, ctx: &egui::Context) {
        let Some(handle) = self.setup_probe.take() else {
            return;
        };
        if !handle.is_finished() {
            self.setup_probe = Some(handle);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let result = match handle.join() {
            Ok(ProbeOutput::Scanned(result))
                if !result.cancelled
                    && self.setup_refresh_pending.is_none()
                    && result.snapshot.is_some() =>
            {
                self.start_setup_identity_verification(result);
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Ok(ProbeOutput::Verified(result)) => result,
            Ok(ProbeOutput::Scanned(result)) => {
                super::super::setup_attention::SetupScanVerification {
                    result,
                    machine_setup_tasks: None,
                    persistence_failed: false,
                }
            }
            Err(_) => super::super::setup_attention::SetupScanVerification {
                result: super::super::setup_attention::SetupScanResult {
                    generation: 0,
                    issue: Some(super::super::setup_attention::SetupIssue::provider(
                        "The machine setup check stopped unexpectedly. Retry the check.",
                    )),
                    snapshot: None,
                    endpoint_fingerprint: None,
                    daemon_identity_fingerprint: None,
                    docker_readiness_fingerprint: None,
                    selected_provider: None,
                    cancelled: false,
                },
                machine_setup_tasks: None,
                persistence_failed: false,
            },
        };
        if self.finish_setup_probe_result(result).is_some() {
            self.start_setup_probe(false);
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            if self.setup_retry_requested && self.setup_attention.is_empty() {
                self.toasts
                    .success("Kool.ad/e setup is ready. Retry planning or implementation.");
            } else if self.setup_retry_requested
                && let Some(notice) = self.setup_attention.first()
            {
                self.toasts.warning(notice.next_action);
            } else if self.setup_retry_requested
                && let Some(task) = self
                    .machine_setup_tasks
                    .iter()
                    .find(|task| task.is_active())
            {
                self.toasts.warning(task.next_step.clone());
            }
            self.setup_retry_requested = false;
        }
    }
}

#[cfg(test)]
#[path = "setup_attention/tests.rs"]
mod tests;
