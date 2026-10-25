use super::KooladeApp;
use std::time::Duration;

pub(super) enum ProbeOutput {
    Scanned(super::super::setup_attention::SetupScanResult),
    Verified(super::super::setup_attention::SetupScanVerification),
}

impl KooladeApp {
    fn clear_setup_attention_snapshot(&mut self) {
        self.setup_attention = None;
        self.machine_setup_tasks.clear();
        self.setup_attention_notified = None;
    }

    fn start_setup_probe(&mut self) {
        let generation = self.setup_scan_generation;
        let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancellation = cancellation.clone();
        self.setup_probe_cancel = Some(cancellation);
        self.setup_probe = Some(std::thread::spawn(move || {
            ProbeOutput::Scanned(super::super::setup_attention::scan(
                generation,
                worker_cancellation,
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
        let setup_commit_lock = self.setup_commit_lock.clone();
        // Identity verification can wait on a Docker probe and the local
        // cross-process task lock. Never make the UI wait for that worker.
        let _commit_guard = match setup_commit_lock.try_lock() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(error)) => Some(error.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => None,
        };
        let defer_generation = _commit_guard.is_none() && self.setup_probe.is_some();
        self.clear_setup_attention_snapshot();
        // If verification already owns the commit lock, defer cancellation
        // and generation advancement until it releases the commit boundary.
        if !defer_generation {
            self.setup_scan_generation = self.setup_scan_generation.saturating_add(1);
            if let Some(cancellation) = &self.setup_probe_cancel {
                cancellation.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        if self.setup_probe.is_some() {
            self.setup_refresh_pending = Some(self.setup_refresh_pending.unwrap_or(false) || retry);
            return;
        }
        self.setup_refresh_pending = None;
        self.setup_retry_requested = retry;
        self.start_setup_probe();
    }

    fn finish_setup_probe_result(
        &mut self,
        verification: super::super::setup_attention::SetupScanVerification,
    ) -> Option<bool> {
        let result = verification.result;
        self.setup_probe_cancel = None;
        let retry = self.setup_refresh_pending.take();
        if result.cancelled || result.generation != self.setup_scan_generation || retry.is_some() {
            if result.generation == self.setup_scan_generation {
                self.setup_scan_generation = self.setup_scan_generation.saturating_add(1);
            }
            self.clear_setup_attention_snapshot();
            self.setup_retry_requested = retry.unwrap_or(self.setup_retry_requested);
            return Some(self.setup_retry_requested);
        }

        if result.snapshot.is_none() {
            self.setup_attention = result.issue.or_else(|| {
                Some(super::super::setup_attention::SetupIssue::provider(
                    "The machine setup check did not finish. Retry the check.",
                ))
            });
            return None;
        }

        self.setup_attention = result.issue;
        if verification.persistence_failed {
            self.setup_attention = Some(super::super::setup_attention::SetupIssue::provider(
                "Could not safely save local machine setup diagnostics. Retry the check.",
            ));
        } else if let Some(tasks) = verification.machine_setup_tasks {
            self.machine_setup_tasks = tasks;
        } else {
            self.setup_attention = Some(super::super::setup_attention::SetupIssue::provider(
                "The machine setup result was not persisted. Retry the check.",
            ));
        }
        None
    }

    pub(super) fn poll_setup_attention(&mut self, ctx: &egui::Context) {
        let Some(handle) = self.setup_probe.take() else {
            return;
        };
        if handle.is_finished() {
            let result = match handle.join() {
                Ok(ProbeOutput::Scanned(result))
                    if !result.cancelled
                        && result.generation == self.setup_scan_generation
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
                        generation: self.setup_scan_generation,
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
                self.start_setup_probe();
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
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
#[path = "setup_attention/tests.rs"]
mod tests;
