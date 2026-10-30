use super::*;

impl KooladeApp {
    pub(super) fn poll_clone_job(&mut self) {
        // Clone job: drain BEFORE Phase 1's screen borrow so a finished
        // worker can refill `conn_path` and drive submit_connect — the
        // single connect authority. Unfinished workers ride back until
        // they settle (poll rhythm: take → is_finished → put back).
        if let Some(job) = self.clone_job.take() {
            if job.join.is_finished() {
                match job.join.join() {
                    Err(_) => {
                        self.conn_error =
                            Some("The clone worker stopped unexpectedly. Try again.".into());
                        self.mirror_clone_failure_if_intent_pending();
                    }
                    Ok(Err(e)) => {
                        // Failed clone: `conn_github` is PRESERVED so the
                        // operator can correct and retry; no connect runs.
                        self.conn_error = Some(Self::conn_banner(&e));
                        self.mirror_clone_failure_if_intent_pending();
                    }
                    Ok(Ok(dest)) => {
                        // Defensive: no navigation exists out of Welcome
                        // while a job runs, so a non-Welcome screen can
                        // only mean something odd — discard silently.
                        if matches!(self.screen, Screen::Welcome) {
                            self.conn_error = None;
                            // Wizard-launched clones carry a pairing
                            // intent: add the planning sibling NOW,
                            // before anything connects.
                            if let Some(intent) = self.pending_clone_pairing.take() {
                                let code_name = dest
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("repository")
                                    .to_string();
                                let planning = if intent.packet_name.trim().is_empty() {
                                    format!("{code_name}.packet")
                                } else {
                                    intent.packet_name.trim().to_string()
                                };
                                let project = if intent.project_name.trim().is_empty() {
                                    code_name.clone()
                                } else {
                                    intent.project_name
                                };
                                let mut spec = crate::core::managed_workspace::SiblingPairSpec::new(
                                    project, code_name,
                                );
                                spec.code_already_exists = true;
                                spec.planning_repo_name = planning;
                                spec.parent_dir = dest
                                    .parent()
                                    .map(std::path::Path::to_path_buf)
                                    .unwrap_or_default();
                                match crate::app::welcome::attempt_create_project(&spec) {
                                    Ok(project) => self.finish_connect(Ok(project)),
                                    // The code clone succeeded; never
                                    // half-connect a checkout with no
                                    // planning peer.
                                    Err(err) => self.wiz_error(format!(
                                        "Clone finished at {} but the planning pair failed:\n{err}",
                                        dest.display(),
                                    )),
                                }
                                return;
                            }
                            // Legacy completion: nothing pairs, connect.
                            self.conn_path = dest.to_string_lossy().into_owned();
                            self.submit_connect();
                        } else {
                            // Forward insurance: no navigation out of
                            // Welcome exists while a job runs, but a
                            // lingering pairing intent must never attach to
                            // a later clone.
                            self.pending_clone_pairing = None;
                        }
                    }
                }
            } else {
                self.clone_job = Some(job);
            }
        }
    }

    /// Wizard-launched clones carry a pending pairing intent; surface worker
    /// failures on the wizard itself so the diagnosis is not visible only in
    /// the classic card's banner beneath.
    fn mirror_clone_failure_if_intent_pending(&mut self) {
        if self.pending_clone_pairing.is_some() {
            let detail = self.conn_error.clone().unwrap_or_default();
            self.wiz_error(format!(
                "The clone did not complete:\n{detail}\nYour inputs are kept — fix the issue, press \u{201c}Continue\u{201d} to return to the confirm step, and press \u{201c}Set up and connect\u{201d} again."
            ));
        }
    }
}
