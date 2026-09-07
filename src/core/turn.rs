//! One planning turn, orchestrated off the UI thread (SPECIFICATION.md
//! §12–§13, §16–§20). Pipeline:
//!
//!   snapshot state ▶ context ▶ prompt ▶ harness (pi) ▶ extract envelope
//!   ▶ validate ▶ apply (+ownership synthesis) ▶ atomic writes ▶ git checkpoint
//!
//! Exactly one worker exists at a time (the UI refuses concurrent submits),
//! so the worker may freely mutate its private state clone; the UI adopts
//! the resulting `PlannerState` when `Applied` lands. Cancellation is
//! cooperative (flag polled between stream events by the harness driver).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::core::apply::{apply, ApplyReceipt};
use crate::core::context_build::TurnContext;
use crate::core::gitops;
use crate::core::prompt::{self, SYSTEM_INSTRUCTIONS};
use crate::core::state::PlannerState;
use crate::core::validation::{self, NormalizedTurn};
use crate::error::AppError;
use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest, TurnEnvelope};

/// Wall-clock budget per turn. Generous on purpose — investigations
/// legitimately take minutes; the Cancel button is the escape hatch.
pub const TURN_TIMEOUT: Duration = Duration::from_secs(600);

/// Immutable inputs frozen when the turn starts; chat is snapshotted so late
/// UI typing cannot race the in-flight prompt.
#[derive(Clone)]
pub struct TurnInputs {
    pub state: PlannerState,
    pub user_message: String,
    /// (speaker, text) oldest → newest, in chat-log order.
    pub recent_chat: Vec<(String, String)>,
}

pub enum TurnEvt {
    /// Live progress preview (tool activity from the harness).
    Activity(String),
    Done(TurnOutcome),
}

#[derive(Debug)]
pub enum TurnOutcome {
    /// Success: artifacts written; `commit_result` carries the checkpoint sha
    /// or the git error. Adoption proceeds either way — the files on disk
    /// already reflect the turn, and the state clone matches disk.
    Applied {
        state: PlannerState,
        receipt: ApplyReceipt,
        normalized: NormalizedTurn,
        commit_result: Result<String, AppError>,
        elapsed: Duration,
        stderr_tail: String,
    },
    /// Envelope rejected by validation: NOTHING written, no commit (§16).
    Rejected {
        problems: Vec<String>,
        final_text: String,
        elapsed: Duration,
    },
    HarnessFailed {
        error: AppError,
        elapsed: Duration,
    },
}

/// Lifetime handle for one running turn; polled from the UI tick loop.
pub struct TurnController {
    rx: Receiver<TurnEvt>,
    /// Held (deliberately unread) so the channel stays open while a turn runs.
    #[allow(dead_code)]
    keepalive: Option<Sender<TurnEvt>>,
    pub cancel_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl TurnController {
    pub fn start(inputs: TurnInputs, harness: Box<dyn AiHarness>) -> TurnController {
        let (evt_tx, evt_rx) = channel();
        let (act_tx, act_rx) = channel::<String>();
        let cancel = Arc::new(AtomicBool::new(false));

        // Forward harness activity into the event stream (de-duplicated).
        {
            let fwd = evt_tx.clone();
            std::thread::spawn(move || {
                let mut last = String::new();
                for line in act_rx {
                    if line == last {
                        continue;
                    }
                    last = line.clone();
                    if fwd.send(TurnEvt::Activity(line)).is_err() {
                        break;
                    }
                }
            });
        }

        let worker_cancel = cancel.clone();
        let worker_evt_tx = evt_tx.clone();
        let worker = std::thread::spawn(move || {
            let outcome = run_turn(&inputs, &*harness, &worker_cancel, act_tx);
            let _ = worker_evt_tx.send(TurnEvt::Done(outcome));
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

fn run_turn(
    inputs: &TurnInputs,
    harness: &dyn AiHarness,
    cancel: &Arc<AtomicBool>,
    activity_tx: Sender<String>,
) -> TurnOutcome {
    let started = Instant::now();
    let user = inputs.state.effective_user();
    let ctx = TurnContext::build(&inputs.state, &inputs.user_message, &inputs.recent_chat);
    let request = PlanningRequest {
        repo_root: inputs.state.repo_root.clone(),
        prompt_body: prompt::render_prompt(&ctx),
        system_instructions: SYSTEM_INSTRUCTIONS.to_string(),
        timeout: TURN_TIMEOUT,
        activity_tx,
        cancel: Arc::clone(cancel),
    };

    let outcome: HarnessOutcome = match harness.execute(&request) {
        Ok(o) => o,
        Err(e) => return TurnOutcome::HarnessFailed { error: e, elapsed: started.elapsed() },
    };

    // Decode (or discover the absence of) the structured block.
    match decode_envelope(&outcome.final_text) {
        EnvelopeDecode::Env(env) => {
            // Validate against the PRE-mutation snapshot.
            match validation::validate(&env, &inputs.state, &user) {
                Err(problems) => TurnOutcome::Rejected {
                    problems,
                    final_text: outcome.final_text,
                    elapsed: started.elapsed(),
                },
                Ok(normalized) => {
                    let mut state = inputs.state.clone();
                    let receipt = match apply(&mut state, &normalized) {
                        Ok(rc) => rc,
                        Err(err) => {
                            return TurnOutcome::HarnessFailed {
                                error: AppError::Other(format!("artifact write failed: {err:#}")),
                                elapsed: started.elapsed(),
                            }
                        }
                    };
                    let commit_result = if receipt.repo_relative_paths.is_empty() {
                        Ok(String::new())
                    } else {
                        gitops::commit(&state.repo_root, &receipt.commit_message, &receipt.repo_relative_paths)
                    };
                    TurnOutcome::Applied {
                        state,
                        receipt,
                        normalized,
                        commit_result,
                        elapsed: started.elapsed(),
                        stderr_tail: outcome.stderr_tail,
                    }
                }
            }
        }
        EnvelopeDecode::Absent => TurnOutcome::Rejected {
            problems: vec!["The planner's final message did not include the required structured JSON block, so NO changes were saved. Try sending the request again.".into()],
            final_text: outcome.final_text,
            elapsed: started.elapsed(),
        },
        EnvelopeDecode::Malformed(detail) => TurnOutcome::Rejected {
            problems: vec![format!("Structured JSON block is malformed ({detail}); NO changes were saved.")],
            final_text: outcome.final_text,
            elapsed: started.elapsed(),
        },
    }
}

enum EnvelopeDecode {
    Env(TurnEnvelope),
    Absent,
    Malformed(String),
}

fn decode_envelope(final_text: &str) -> EnvelopeDecode {
    use crate::harness::pi_extract::extract_json_object;
    match extract_json_object(final_text) {
        None => EnvelopeDecode::Absent,
        Some(blob) => match serde_json::from_str::<TurnEnvelope>(&blob) {
            Ok(env) => EnvelopeDecode::Env(env),
            Err(e) => EnvelopeDecode::Malformed(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{TurnItem, HarnessOutcome};

    /// Fake harness for pipeline tests: canned envelopes, zero processes.
    struct ScriptedHarness {
        canned: Option<TurnEnvelope>,
        raw: Option<String>,
    }
    impl AiHarness for ScriptedHarness {
        fn label(&self) -> String {
            "scripted-test".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("test".into())
        }
        fn execute(&self, _req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            let text = match (&self.raw, &self.canned) {
                (Some(raw), _) => raw.clone(),
                (_, Some(env)) => {
                    let json = serde_json::to_string(env).unwrap();
                    format!("Summary text here.\n\n```json\n{json}\n```")
                }
                (None, None) => "(forgetting the block entirely)".into(),
            };
            Ok(HarnessOutcome {
                final_text: text,
                envelope: None, // force the pipeline to extract + decode itself
                stderr_tail: String::new(),
            })
        }
    }

    fn inputs_for(tag: &str, msg: &str) -> (TurnInputs, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("packet_turn_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            ["init"].as_slice(),
            ["config", "user.email", "packet@test.local"].as_slice(),
            ["config", "user.name", "Packet Test"].as_slice(),
        ] {
            let _ = std::process::Command::new("git").args(args).current_dir(&root).output();
        }
        let mut st = PlannerState::load(&root).unwrap();
        st.bootstrap_missing().unwrap();
        let inputs = TurnInputs {
            state: st,
            user_message: msg.into(),
            recent_chat: Vec::new(),
        };
        let dir = inputs.state.repo_root.clone();
        (inputs, dir)
    }

    fn drain(controller: &TurnController) -> TurnOutcome {
        loop {
            match controller.poll(Duration::from_millis(250)) {
                Some(TurnEvt::Activity(_)) => {}
                Some(TurnEvt::Done(o)) => return o,
                None => panic!("turn vanished"),
            }
        }
    }

    #[test]
    fn happy_path_writes_files_and_commits() {
        let (inputs, dir) = inputs_for("happy", "please draft the initial spec");
        let env = TurnEnvelope {
            schema_version: Some(1),
            assistant_message: Some("Drafted an initial spec and raised the first question.".into()),
            change_summary: Some("Draft initial specification".into()),
            updated_specification: Some("# Fixture\n\n## Goals\nDemo the planner end-to-end.\n".into()),
            open_items_added: Some(vec![TurnItem {
                id: None,
                kind: Some("Question".into()),
                category: Some("General".into()),
                assigned_to: Some("All".into()),
                priority: Some("Normal".into()),
                question: Some("Which deployment target first?".into()),
                reason: Some("packaging depends on it".into()),
                resolution_note: None,
            }]),
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: None,
        };
        let c = TurnController::start(inputs, Box::new(ScriptedHarness { canned: Some(env), raw: None }));
        match drain(&c) {
            TurnOutcome::Applied { state, receipt, commit_result, .. } => {
                assert_eq!(receipt.repo_relative_paths.len(), 2);
                assert!(receipt.commit_message.starts_with("planner: "));
                assert!(commit_result.is_ok(), "commit failed: {commit_result:?}");
                assert_eq!(state.items.len(), 1);
            }
            other => panic!("expected Applied, got: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_block_rejects_without_side_effects() {
        let (inputs, dir) = inputs_for("noblock", "go");
        let c = TurnController::start(
            inputs,
            Box::new(ScriptedHarness { canned: None, raw: Some("chatty but no json 😅".into()) }),
        );
        match drain(&c) {
            TurnOutcome::Rejected { problems, .. } => {
                assert!(!problems.is_empty());
            }
            other => panic!("expected Rejected, got: {other:?}"),
        }
        let st = PlannerState::load(&dir).unwrap();
        assert!(st.items.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_envelope_types_reject_cleanly() {
        let (inputs, dir) = inputs_for("badyes", "resolve CLR-999 please");
        // Envelope that claims to resolve an id that does not exist.
        let env = TurnEnvelope {
            schema_version: Some(1),
            assistant_message: Some("Okay!".into()),
            change_summary: None,
            updated_specification: None,
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: Some(vec!["CLR-999".into()]),
            next_question_id: None,
        };
        let c = TurnController::start(inputs, Box::new(ScriptedHarness { canned: Some(env), raw: None }));
        match drain(&c) {
            TurnOutcome::Rejected { problems, .. } => {
                assert!(problems.iter().any(|p| p.contains("CLR-999")));
            }
            other => panic!("expected Rejected, got: {other:?}"),
        }
        let st = PlannerState::load(&dir).unwrap();
        assert!(st.items.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_api_toggles_predictably() {
        let (inputs, dir) = inputs_for("cancel", "think hard");
        let c = TurnController::start(
            inputs,
            Box::new(ScriptedHarness { canned: None, raw: Some("{}\n".into()) }),
        );
        assert!(!c.cancel_requested());
        c.request_cancel();
        assert!(c.cancel_requested());
        let _ = drain(&c);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
