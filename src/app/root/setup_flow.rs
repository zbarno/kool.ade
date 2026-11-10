//! Application glue for the deterministic "Prepare this computer" flow.
//!
//! [`KooladeApp::pump_setup_flow`] runs one pure driver beat at most every
//! `BEAT_EVERY_MS` and translates the resulting action into the existing
//! facilities: scoped repair dispatch, the setup-task store, rescans, and
//! operator-visible activity notes. No repair logic and no provider calls
//! live here; the policy is the driver's.

use super::KooladeApp;
use crate::app::setup_workflow::{
    self as core, BeatInput, FlowAction, SlotFamily, SlotPhase, SlotStatus,
};
use crate::persistence::setup_tasks::{
    DoneOutcomeV1, FlowPhaseV1, HaltReasonV1, LocalTask, SetupFlowStateV1, State,
};

/// Wall-clock spacing between driver beats while the app stays open.
const BEAT_EVERY_MS: u64 = 1_500;

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

impl KooladeApp {
    /// Advance the machine setup flow if a beat is due.
    pub(super) fn pump_setup_flow(&mut self) {
        let now = now_unix_ms();
        let due = self
            .last_setup_flow_beat
            .is_none_or(|last| now.saturating_sub(last) >= BEAT_EVERY_MS);
        self.last_setup_flow_beat = Some(now);
        if !due {
            return;
        }
        self.setup_flow_cache = match crate::persistence::setup_tasks::load_flow() {
            Ok(state) => state,
            // Storage hiccup: keep the last good snapshot, retry next beat.
            Err(_) => self.setup_flow_cache.clone(),
        };
        let Some(mut state) = self.setup_flow_cache.clone() else {
            return;
        };
        if self.observe_verifications(&mut state) {
            self.commit_flow(&state);
        }
        self.track_scan_generation(state.last_scan_generation);
        if !self.setup_flow_boot_synced {
            self.setup_flow_boot_synced = true;
            if matches!(state.phase, FlowPhaseV1::Running | FlowPhaseV1::Finishing,) {
                state.phase = FlowPhaseV1::Halted {
                    reason: HaltReasonV1::InterruptedByRestart,
                    detail_task: None,
                };
                self.commit_flow(&state);
                self.setup_flow_note(
                    "Setup preparation picked up after a restart and is waiting for your go-ahead.",
                );
                return;
            }
        }
        if state.phase.is_terminal() {
            return;
        }
        let generation = self
            .machine_setup_tasks
            .iter()
            .map(|task| task.scan_generation)
            .max()
            .unwrap_or(0);
        let slots = [
            occupy(SlotFamily::Remediation, self.setup_remediation.is_busy()),
            occupy(SlotFamily::Image, self.setup_image_preparation.is_busy()),
            occupy(SlotFamily::ClaudeInstall, self.claude_install_running()),
        ];
        let action = core::beat(&BeatInput {
            flow: Some(&state),
            tasks: &self.machine_setup_tasks,
            scan_generation: generation,
            probe_busy: self.setup_probe.is_some(),
            slots: &slots,
            needs_fresh_scan: !self.machine_setup_scan_current,
            now_unix_ms: now,
            rescan_cooldown_ms: core::DEFAULT_RESCAN_COOLDOWN_MS,
            last_rescan_at: self.setup_flow_rescanned_at,
        });
        self.apply_flow_action(action, &mut state, now);
    }

    fn apply_flow_action(&mut self, action: FlowAction, state: &mut SetupFlowStateV1, now: u64) {
        match action {
            FlowAction::Hold => {}
            FlowAction::BeginFix { task_id, .. } => self.request_setup_task_fix(&task_id),
            FlowAction::ConsentOutstanding { task_id } => {
                state.advance(now, |state| {
                    let already = matches!(
                        state.phase,
                        FlowPhaseV1::WaitingForConsent {
                            task_id: ref current
                        } if current.as_str() == task_id
                    );
                    if !already {
                        state.phase = FlowPhaseV1::WaitingForConsent { task_id };
                    }
                    true
                });
                self.commit_flow(state);
            }
            FlowAction::SyncPhase { phase, .. } => {
                if state.phase == phase {
                    return;
                }
                state.advance(now, |state| {
                    state.phase = phase;
                    true
                });
                self.commit_flow(state);
            }
            FlowAction::TriggerRescan => {
                self.setup_flow_rescanned_at = Some(now);
                self.refresh_setup_attention(false);
            }
            FlowAction::Finish(outcome) => {
                state.phase = FlowPhaseV1::Complete(outcome);
                self.commit_flow(state);
                let message = match outcome {
                    DoneOutcomeV1::FullyPrepared => {
                        "This computer is fully prepared for coding tasks.".to_owned()
                    }
                    DoneOutcomeV1::PreparedWithUnavailable { unavailable_count } => format!(
                        "This computer is prepared. {unavailable_count} check(s) cannot run \
                         on this operating system and stay marked unavailable."
                    ),
                };
                self.setup_flow_note(&message);
                self.toasts
                    .success("Machine setup finished. See the setup checklist for the result.");
            }
            FlowAction::Halt {
                reason,
                detail_task,
            } => {
                state.phase = FlowPhaseV1::Halted {
                    reason,
                    detail_task,
                };
                self.commit_flow(state);
                self.setup_flow_note(&halt_note(
                    reason,
                    state.phase.task_reference(),
                    &self.machine_setup_tasks,
                ));
            }
        }
    }

    /// Record tasks that just became verified so readiness-regression
    /// detection has truthful ground truth. Reports whether the state
    /// changed so the caller can persist it.
    fn observe_verifications(&self, state: &mut SetupFlowStateV1) -> bool {
        let newly = self
            .machine_setup_tasks
            .iter()
            .filter(|task| task.state == State::Verified && !state.seen_verified.contains(&task.id))
            .map(|task| task.id.clone())
            .collect::<Vec<_>>();
        if newly.is_empty() {
            return false;
        }
        state.seen_verified.extend(newly.clone());
        state.last_completed.extend(newly);
        true
    }

    fn track_scan_generation(&mut self, observed: u64) {
        let recorded = self.setup_flow_seen_generation.get_or_insert(observed);
        if *recorded != observed {
            *recorded = observed;
            // A newer authoritative scan arrived; the cooldown window that
            // gated the previous rescan no longer applies.
            self.setup_flow_rescanned_at = None;
        }
    }

    fn commit_flow(&mut self, state: &SetupFlowStateV1) {
        let snapshot = state.clone();
        match crate::persistence::setup_tasks::update_flow(|stored| {
            *stored = snapshot.clone();
            true
        }) {
            Ok(Some(stored)) => self.setup_flow_cache = Some(stored),
            _ => self.setup_flow_cache = Some(snapshot),
        }
    }

    fn setup_flow_note(&mut self, message: &str) {
        if let super::Screen::Connected(project) = &mut self.screen {
            project.activity.pending.push(message.to_owned());
        }
    }

    fn claude_install_running(&self) -> bool {
        matches!(
            self.claude_install.status(),
            crate::app::bootstrap_remediation::ClaudeInstallStatus::NeedsApproval
                | crate::app::bootstrap_remediation::ClaudeInstallStatus::Running
        )
    }

    /// Operator command: start (fresh flow), resume (paused/halted), or
    /// no-op when the flow is already moving or finished.
    pub(super) fn start_setup_flow_cmd(&mut self) {
        let now = now_unix_ms();
        match crate::persistence::setup_tasks::load_flow() {
            Ok(None) | Err(_) => self.begin_fresh_flow(now),
            Ok(Some(current)) if !current.phase.is_terminal() => {
                let mut next = current;
                if matches!(next.phase, FlowPhaseV1::Paused | FlowPhaseV1::Halted { .. }) {
                    next.advance(now, |state| {
                        state.phase = FlowPhaseV1::Running;
                        true
                    });
                    self.commit_flow(&next);
                    self.setup_flow_note("Setup preparation continued.");
                }
                self.setup_flow_cache = Some(next);
            }
            Ok(Some(current)) => {
                // Finished run: honor the "explicit restart" rule by starting
                // a fresh flow with a clean slate of declines.
                self.setup_flow_cache = Some(current);
                self.begin_fresh_flow(now);
            }
        }
    }

    fn begin_fresh_flow(&mut self, now: u64) {
        // Nanosecond grain: millisecond stamps collide when a fresh flow is
        // started shortly after another one (e.g. stop then immediately
        // restart), and colliding ids blur the audit trail.
        let unix_nanoseconds = std::time::UNIX_EPOCH.elapsed().map_or(0_u64, |elapsed| {
            elapsed.as_secs().saturating_mul(1_000_000_000) + elapsed.subsec_nanos() as u64
        });
        static BEGIN_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = BEGIN_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let flow_id = format!("prep-{unix_nanoseconds:012x}-{sequence:04x}");
        let Some(fresh) = SetupFlowStateV1::start_new(flow_id, now).ok() else {
            self.toasts
                .warning("Could not start setup preparation. Try again.");
            return;
        };
        match crate::persistence::setup_tasks::insert_or_replace_terminal_flow(fresh.clone()) {
            Ok(true) | Ok(false) => {}
            Err(error) => {
                self.toasts
                    .warning(format!("Could not start setup preparation: {error}"));
                return;
            }
        }
        self.setup_flow_cache = crate::persistence::setup_tasks::load_flow()
            .ok()
            .flatten()
            .or(Some(fresh));
        self.setup_flow_boot_synced = true;
        self.setup_flow_rescanned_at = None;
        self.setup_flow_note(
            "Setup preparation started. Anything that touches the machine is explained and approved separately.",
        );
    }

    /// Operator command: park the flow. In-flight work keeps going to a
    /// safe settle; nothing new begins while paused.
    pub(super) fn pause_setup_flow_cmd(&mut self) {
        let now = now_unix_ms();
        let Some(current) = crate::persistence::setup_tasks::load_flow().ok().flatten() else {
            return;
        };
        if current.phase.is_terminal() || matches!(current.phase, FlowPhaseV1::Paused) {
            self.setup_flow_cache = Some(current);
            return;
        }
        let mut paused = current;
        paused.advance(now, |state| {
            state.phase = FlowPhaseV1::Paused;
            true
        });
        self.commit_flow(&paused);
        self.setup_flow_note("Setup preparation paused. Resume it when ready.");
    }

    /// Operator command: stop outright; resumable, never destructive.
    pub(super) fn stop_setup_flow_cmd(&mut self) {
        let now = now_unix_ms();
        let Some(current) = crate::persistence::setup_tasks::load_flow().ok().flatten() else {
            return;
        };
        if current.phase.is_terminal() {
            self.setup_flow_cache = Some(current);
            return;
        }
        let mut stopped = current;
        stopped.advance(now, |state| {
            state.phase = FlowPhaseV1::Halted {
                reason: HaltReasonV1::StoppedByOperator,
                detail_task: None,
            };
            true
        });
        self.commit_flow(&stopped);
        self.setup_flow_note("Setup preparation stopped. Resume it when ready.");
    }

    /// Operator command: decline one offered operation. The flow stops
    /// proposing it automatically (until an explicit restart), and any
    /// staging consent for it is withdrawn so the approval cannot linger.
    pub(super) fn decline_setup_operation_cmd(&mut self, task_id: &str) {
        let now = now_unix_ms();
        let Some(mut declined) = crate::persistence::setup_tasks::load_flow().ok().flatten() else {
            self.toasts
                .warning("No setup preparation is in progress to decline from.");
            return;
        };
        declined.advance(now, |state| {
            if !state.skipped.iter().any(|known| known.as_str() == task_id) {
                state.skipped.push(task_id.to_owned());
            }
            if matches!(
                state.phase,
                FlowPhaseV1::WaitingForConsent {
                    task_id: ref waiting
                } if waiting.as_str() == task_id
            ) {
                state.phase = FlowPhaseV1::Running;
            }
            true
        });
        withdraw_staging_review(self, task_id);
        self.commit_flow(&declined);
        self.setup_flow_note(&decline_note(task_id, &self.machine_setup_tasks));
    }
}

fn withdraw_staging_review(app: &mut KooladeApp, task_id: &str) {
    let route = app
        .machine_setup_tasks
        .iter()
        .find(|task| task.id == task_id)
        .and_then(core::fix_route);
    match route {
        Some(core::FixRoute::Remediation) => app.setup_remediation.cancel_review(),
        Some(core::FixRoute::ClaudeInstall) => {
            let _ = app.claude_install.cancel_review();
        }
        Some(core::FixRoute::Image) | None => app.setup_image_preparation.cancel_review(),
    }
    app.publish_setup_task_store_update();
}

fn decline_note(task_id: &str, tasks: &[LocalTask]) -> String {
    let title = tasks
        .iter()
        .find(|task| task.id == task_id)
        .map(crate::ui::layout::setup_task_title)
        .unwrap_or_else(|| "that setup change".to_owned());
    format!(
        "Understood — {title} is skipped for now. \
         Say the word and it can be retried manually."
    )
}

fn occupy(family: SlotFamily, busy: bool) -> SlotStatus {
    SlotStatus {
        family: Some(family),
        task_id: None,
        phase: if busy {
            SlotPhase::Running
        } else {
            SlotPhase::Idle
        },
    }
}

fn halt_note(reason: HaltReasonV1, reference: Option<&str>, tasks: &[LocalTask]) -> String {
    let title = reference
        .and_then(|id| {
            tasks
                .iter()
                .find(|task| task.id == id)
                .map(crate::ui::layout::setup_task_title)
        })
        .unwrap_or_else(|| "an earlier setup step".to_owned());
    match reason {
        HaltReasonV1::ReadinessChanged => format!(
            "Setup preparation paused for review: {title} no longer checks as ready. \
             Nothing changed on your machine — resume after looking it over."
        ),
        HaltReasonV1::InterruptedByRestart => {
            "Setup preparation paused after the app restarted. Resume it when ready.".to_owned()
        }
        HaltReasonV1::StoppedByOperator => {
            "Setup preparation stopped. Resume it when ready.".to_owned()
        }
        HaltReasonV1::RepeatedFailures => format!(
            "Setup preparation stopped after repeated trouble near {title}. \
             Review it, then retry that step manually."
        ),
        _ => "Setup preparation stopped and is waiting. Check the setup checklist, then resume when ready.".to_owned(),
    }
}

#[cfg(test)]
mod tests;
