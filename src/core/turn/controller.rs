use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::core::state::PlannerState;
use crate::domain::Authority;
use crate::harness::{AiHarness, LiveProgress};

use super::execute::run_turn;
use super::{TurnEvt, TurnInputs, TurnOutcome, configured_turn_timeout};

/// Lifetime handle for one running turn; polled from the UI tick loop.
pub struct TurnController {
    rx: Receiver<TurnEvt>,
    /// Held (deliberately unread) so the channel stays open while a turn runs.
    #[allow(dead_code)]
    keepalive: Option<Sender<TurnEvt>>,
    pub cancel_flag: Arc<AtomicBool>,
    pub(super) worker: Option<JoinHandle<()>>,
}

impl TurnController {
    pub fn start(inputs: TurnInputs, harness: Box<dyn AiHarness>) -> TurnController {
        Self::start_scoped(inputs, harness, None)
    }

    pub fn start_scoped(
        inputs: TurnInputs,
        harness: Box<dyn AiHarness>,
        task: Option<String>,
    ) -> TurnController {
        let (evt_tx, evt_rx) = channel();
        let (act_tx, act_rx) = channel::<LiveProgress>();
        let cancel = Arc::new(AtomicBool::new(false));

        // Forward snapshots in order and drain them before sending Done.
        let forwarder = {
            let fwd = evt_tx.clone();
            std::thread::spawn(move || {
                let mut last = LiveProgress::default();
                for line in act_rx {
                    if line == last {
                        continue;
                    }
                    last = line.clone();
                    if fwd.send(TurnEvt::Progress(Box::new(line))).is_err() {
                        break;
                    }
                }
            })
        };

        let worker_cancel = cancel.clone();
        let worker_evt_tx = evt_tx.clone();
        let worker = std::thread::spawn(move || {
            let budget = configured_turn_timeout();
            let began = Instant::now();
            let mut inputs = inputs;
            let mut outcome = run_turn(
                &inputs,
                &*harness,
                &worker_cancel,
                act_tx.clone(),
                task.as_deref(),
                budget,
            );
            // Independent chats may finish against the same base. Re-plan
            // against current artifacts after contention; never apply a stale
            // replacement or ask the user to repeat already saved input.
            for _ in 0..3 {
                let drift = matches!(&outcome, TurnOutcome::Rejected { problems, .. }
                    if problems.iter().any(|p| p.starts_with("Planning files changed on disk")));
                if !drift || worker_cancel.load(Ordering::SeqCst) {
                    break;
                }
                let Some(remaining) = budget.checked_sub(began.elapsed()).filter(|d| !d.is_zero())
                else {
                    break;
                };
                let Ok(current) = PlannerState::load(&inputs.state.repo_root) else {
                    break;
                };
                inputs.state = current;
                let _ = act_tx.send(LiveProgress {
                    activity: Some(
                        "Refreshing planning context after another conversation saved…".into(),
                    ),
                    ..Default::default()
                });
                outcome = run_turn(
                    &inputs,
                    &*harness,
                    &worker_cancel,
                    act_tx.clone(),
                    task.as_deref(),
                    remaining,
                );
            }
            drop(act_tx);
            let _ = forwarder.join();
            let _ = worker_evt_tx.send(TurnEvt::Done(Box::new(outcome)));
        });

        TurnController {
            rx: evt_rx,
            keepalive: Some(evt_tx),
            cancel_flag: cancel,
            worker: Some(worker),
        }
    }

    /// Cooperative cancel: the harness notices between stream events.
    pub fn request_cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub fn cancel_requested(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }

    /// Poll for the next event, bounded by `wait` so the UI stays responsive.
    pub fn poll(&self, wait: Duration) -> Option<TurnEvt> {
        self.rx.recv_timeout(wait).ok()
    }
}

pub(super) fn user_replied_human_item_ids(state: &PlannerState, task: Option<&str>) -> Vec<String> {
    let Some(scope) = task else {
        return Vec::new();
    };
    state
        .items
        .iter()
        .find(|item| item.conversation_key() == scope && item.authority == Authority::Human)
        .map(|item| vec![item.id.clone()])
        .unwrap_or_default()
}

impl Drop for TurnController {
    fn drop(&mut self) {
        // Guarantee the harness child dies even if the UI abandoned the turn.
        self.request_cancel();
        if let Some(w) = self.worker.take() {
            let _ = w.thread().id();
            // Detached deliberately: a wedged child must not freeze the UI
            // thread. Process death is guaranteed by the cancel flag plus
            // ChildTask's kill-on-drop guard.
        }
    }
}
