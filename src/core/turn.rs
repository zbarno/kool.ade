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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::core::apply::{ApplyReceipt, apply};
use crate::core::context_build::TurnContext;
use crate::core::gitops;
use crate::core::prompt::{self, SYSTEM_INSTRUCTIONS};
use crate::core::state::PlannerState;
use crate::core::validation::{self, NormalizedTurn};
use crate::error::AppError;
use crate::harness::{AiHarness, HarnessOutcome, LiveProgress, PlanningRequest, TurnEnvelope};

/// Local inference can take hours; silence never shortens this deadline.
/// The user can still stop a running turn with Cancel.
pub const TURN_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);

/// Optional positive wall-clock budget in seconds, read when a turn begins.
/// Invalid, zero, or unrepresentable values fall back to the twelve-hour default.
pub fn configured_turn_timeout() -> Duration {
    std::env::var("PACKET_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .filter(|duration| Instant::now().checked_add(*duration).is_some())
        .unwrap_or(TURN_TIMEOUT)
}

/// Immutable inputs frozen when the turn starts; chat is snapshotted so late
/// UI typing cannot race the in-flight prompt.
#[derive(Clone)]
pub struct TurnInputs {
    pub state: PlannerState,
    pub user_message: String,
    /// (speaker, text) oldest → newest, in chat-log order.
    pub recent_chat: Vec<(String, String)>,
    pub purpose: crate::core::workflow::TurnPurpose,
}

pub enum TurnEvt {
    /// Live display snapshot from the harness; never authoritative state.
    Progress(LiveProgress),
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
                    if fwd.send(TurnEvt::Progress(line)).is_err() {
                        break;
                    }
                }
            })
        };

        let worker_cancel = cancel.clone();
        let worker_evt_tx = evt_tx.clone();
        let worker = std::thread::spawn(move || {
            let outcome = run_turn(&inputs, &*harness, &worker_cancel, act_tx);
            let _ = forwarder.join();
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
    progress_tx: Sender<LiveProgress>,
) -> TurnOutcome {
    let started = Instant::now();
    let user = inputs.state.effective_user();
    let ctx = TurnContext::build(&inputs.state, &inputs.user_message, &inputs.recent_chat);
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks
        && !inputs
            .state
            .workflow
            .ready(inputs.state.planning_contract())
    {
        return TurnOutcome::Rejected {
            problems: vec!["Continue the interview and approve task generation first.".into()],
            final_text: String::new(),
            elapsed: started.elapsed(),
        };
    }
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks {
        if let Some((id, _)) = &inputs.state.active_feature {
            if !crate::core::workflow::feature_approved(
                &inputs.state.repo_root,
                &inputs.state.workflow,
                id,
            ) {
                return TurnOutcome::Rejected {
                    problems: vec![format!(
                        "{id} needs explicit implementation approval before task generation"
                    )],
                    final_text: String::new(),
                    elapsed: started.elapsed(),
                };
            }
        }
    }
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks {
        match PlannerState::load(&inputs.state.repo_root) {
            Ok(current) if current.spec_text == inputs.state.spec_text
                && current.active_feature == inputs.state.active_feature
                && current.repositories == inputs.state.repositories
                && current.workflow == inputs.state.workflow && current.items == inputs.state.items => {}
            _ => return TurnOutcome::Rejected { problems: vec!["Planning files changed since the readiness offer. Reopen the repository and review the current plan before generating tasks.".into()], final_text: String::new(), elapsed: started.elapsed() },
        }
    }
    let mut prompt_body = prompt::render_prompt(&ctx);
    prompt_body.push_str(&prompt::workflow_context(&inputs.state, inputs.purpose));
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks {
        prompt_body.push_str(prompt::TASK_OUTLINE_STEP);
    }
    let request = PlanningRequest {
        implementation: false,
        read_only: false,
        repo_root: inputs.state.repo_root.clone(),
        prompt_body,
        system_instructions: format!(
            "{SYSTEM_INSTRUCTIONS}\n{}\n{}",
            prompt::SPECIFICATION_POLICY,
            prompt::WORKFLOW_INSTRUCTIONS
        ),
        timeout: configured_turn_timeout(),
        progress_tx,
        cancel: Arc::clone(cancel),
    };

    let result = if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks {
        crate::core::task_generation::generate(harness, &request, &inputs.state, started)
    } else {
        harness.execute(&request)
    };
    let outcome: HarnessOutcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            return TurnOutcome::HarnessFailed {
                error,
                elapsed: started.elapsed(),
            };
        }
    };
    if cancel.load(Ordering::SeqCst) {
        return TurnOutcome::HarnessFailed {
            error: AppError::HarnessFailed {
                reason: "cancelled by user".into(),
                stderr_tail: String::new(),
            },
            elapsed: started.elapsed(),
        };
    }
    // Decode (or discover the absence of) the structured block.
    match decode_envelope(&outcome.final_text) {
        EnvelopeDecode::Env(env) => {
            // Validate against the PRE-mutation snapshot.
            match validation::validate_for_turn(&env, &inputs.state, &user, inputs.purpose) {
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
    use crate::artifacts::config_io;
    use crate::domain::{CategoryOwners, ItemKind, Stakeholders};
    use crate::harness::{HarnessOutcome, TurnItem};

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
        fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            assert!(
                req.system_instructions
                    .contains(prompt::SPECIFICATION_POLICY)
            );
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
            let _ = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output();
        }
        let mut st = PlannerState::load(&root).unwrap();
        st.bootstrap_missing().unwrap();
        let inputs = TurnInputs {
            state: st,
            user_message: msg.into(),
            recent_chat: Vec::new(),
            purpose: crate::core::workflow::TurnPurpose::Interview,
        };
        let dir = inputs.state.repo_root.clone();
        (inputs, dir)
    }

    fn drain(controller: &TurnController) -> TurnOutcome {
        loop {
            match controller.poll(Duration::from_millis(250)) {
                Some(TurnEvt::Progress(_)) => {}
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
            assistant_message: Some(
                "Drafted an initial spec and raised the first question.".into(),
            ),
            change_summary: Some("Draft initial specification".into()),
            document_updates: None,
            updated_specification: Some(crate::core::specification::fixture(
                "Demo the planner end-to-end.",
            )),
            open_items_added: Some(vec![TurnItem {
                authority: None,
                id: None,
                kind: Some("Question".into()),
                category: Some("General".into()),
                assigned_to: Some("All".into()),
                priority: Some("Normal".into()),
                question: Some("Which deployment target first?".into()),
                reason: Some("packaging depends on it".into()),
                resolution_note: None,
                feature_id: None,
                recommendation: None,
                evidence: None,
            }]),
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: None,
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let c = TurnController::start(
            inputs,
            Box::new(ScriptedHarness {
                canned: Some(env),
                raw: None,
            }),
        );
        match drain(&c) {
            TurnOutcome::Applied {
                state,
                receipt,
                commit_result,
                ..
            } => {
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
    fn modular_turn_changes_only_named_modules_and_rejects_bad_id_atomically() {
        let (mut inputs, dir) = inputs_for("modular_turn", "refine product scope");
        let legacy = std::fs::read_to_string(dir.join("planning/specification.md")).unwrap();
        crate::artifacts::product_docs::migrate(&dir, &legacy).unwrap();
        git_stdout(&dir, &["add", "-A"]);
        git_stdout(&dir, &["commit", "-m", "Seed modular product"]);
        inputs.state = PlannerState::load(&dir).unwrap();
        let vision = dir.join("planning/product/01-vision.md");
        let scope = dir.join("planning/product/02-scope.md");
        let unrelated = dir.join("planning/product/03-actors-and-roles.md");
        let original_unrelated = std::fs::read(&unrelated).unwrap();
        let changed_vision = "## 1. Vision\n\nCurrent purpose from inspected evidence.\n";
        let changed_scope = "## 2. Scope\n\nCurrent scope from accepted intent.\n";
        let env = TurnEnvelope {
            schema_version: Some(2),
            assistant_message: Some("Updated two product areas.".into()),
            change_summary: Some("Refine product vision and scope".into()),
            document_updates: Some(vec![
                crate::harness::DocumentUpdate {
                    document_id: "product:01-vision".into(),
                    content: changed_vision.into(),
                },
                crate::harness::DocumentUpdate {
                    document_id: "product:02-scope".into(),
                    content: changed_scope.into(),
                },
            ]),
            updated_specification: None,
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: None,
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let result = drain(&TurnController::start(
            inputs.clone(),
            Box::new(ScriptedHarness {
                canned: Some(env),
                raw: None,
            }),
        ));
        match result {
            TurnOutcome::Applied {
                receipt,
                commit_result,
                ..
            } => {
                assert!(commit_result.is_ok());
                assert_eq!(receipt.repo_relative_paths.len(), 2);
            }
            other => panic!("expected modular apply, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&vision).unwrap(), changed_vision);
        assert_eq!(std::fs::read_to_string(&scope).unwrap(), changed_scope);
        assert_eq!(std::fs::read(&unrelated).unwrap(), original_unrelated);
        let before_commit = git_stdout(&dir, &["rev-list", "--count", "HEAD"]);
        let before_vision = std::fs::read(&vision).unwrap();
        inputs.state = PlannerState::load(&dir).unwrap();
        let bad = serde_json::json!({"schema_version":2,"assistant_message":"Changed scope",
            "document_updates":[{"document_id":"product:01-vision","content":"## 1. Vision\n\nWrong\n"},
                {"document_id":"product:../../escape","content":"bad"}],
            "open_items_added":[{"kind":"Question","priority":"Normal","authority":"Human",
                "category":"General","assigned_to":"All","question":"Should this ship?","reason":"Release decision"}]}).to_string();
        let result = drain(&TurnController::start(
            inputs,
            Box::new(ScriptedHarness {
                canned: None,
                raw: Some(bad),
            }),
        ));
        assert!(matches!(result, TurnOutcome::Rejected { .. }));
        assert_eq!(std::fs::read(&vision).unwrap(), before_vision);
        assert_eq!(std::fs::read(&unrelated).unwrap(), original_unrelated);
        assert_eq!(
            git_stdout(&dir, &["rev-list", "--count", "HEAD"]),
            before_commit
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn modular_turn_creates_next_feature_and_indexes_it() {
        let (mut inputs, dir) = inputs_for("new_feature", "Plan saved searches");
        let legacy = std::fs::read_to_string(dir.join("planning/specification.md")).unwrap();
        crate::artifacts::product_docs::migrate(&dir, &legacy).unwrap();
        git_stdout(&dir, &["add", "-A"]);
        git_stdout(&dir, &["commit", "-m", "Seed modular product"]);
        inputs.state = PlannerState::load(&dir).unwrap();
        let feature = "# CHG-001: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave repeated searches.\n\n## Current Behavior\n\nNo saved searches observed.\n\n## Desired Behavior\n\nUsers can save searches.\n\n## Scope\n\nSearch UI only.\n\n## Affected Product Areas\n\n`product:05-functional-requirements`\n\n## Requirements\n\nSave and restore.\n\n## Decisions and Assumptions\n\nNone yet.\n\n## Acceptance Criteria\n\nA saved search reopens.\n";
        let env = TurnEnvelope {
            schema_version: Some(2),
            assistant_message: Some("Drafted saved searches.".into()),
            change_summary: Some("Draft saved searches".into()),
            document_updates: Some(vec![crate::harness::DocumentUpdate {
                document_id: "feature:CHG-001".into(),
                content: feature.into(),
            }]),
            updated_specification: None,
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: None,
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let result = drain(&TurnController::start(
            inputs,
            Box::new(ScriptedHarness {
                canned: Some(env),
                raw: None,
            }),
        ));
        match result {
            TurnOutcome::Applied {
                receipt,
                commit_result,
                state,
                ..
            } => {
                assert!(commit_result.is_ok());
                assert_eq!(receipt.repo_relative_paths.len(), 2);
                let contract = crate::core::contract_snapshot::freeze(&state)
                    .unwrap()
                    .unwrap();
                assert_eq!(contract.feature_id, "CHG-001");
                assert!(
                    contract
                        .product_modules
                        .contains_key("05-functional-requirements")
                );
                assert!(contract.repository_bases.contains_key("root"));
            }
            other => panic!("expected new feature, got {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(
                dir.join("planning/features/CHG-001-saved-searches/specification.md")
            )
            .unwrap(),
            feature
        );
        assert!(
            std::fs::read_to_string(dir.join("planning/product/index.md"))
                .unwrap()
                .contains("CHG-001-saved-searches")
        );
        assert_eq!(
            crate::artifacts::product_docs::next_feature_id(&dir),
            "CHG-002"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_specification_rejects_otherwise_valid_turn_without_mutation() {
        let (inputs, dir) = inputs_for("spec_layout", "rewrite the specification");
        git_stdout(&dir, &["add", "planning", ".planner"]);
        git_stdout(&dir, &["commit", "-m", "Seed planning artifacts"]);
        let before = law_evidence(&dir);
        let raw = serde_json::json!({
            "schema_version": 1,
            "assistant_message": "Revised the document.",
            "change_summary": "Revise specification",
            "updated_specification": "# Fixture\n\n## Audit notes\nIncomplete replacement.",
            "open_items_added": [{"kind":"Question", "priority":"Normal",
                "category":"General", "assigned_to":"All", "question":"Which platform?",
                "reason":"Defines launch scope"}]
        })
        .to_string();
        let controller = TurnController::start(
            inputs,
            Box::new(ScriptedHarness {
                canned: None,
                raw: Some(raw),
            }),
        );
        match drain(&controller) {
            TurnOutcome::Rejected { problems, .. } => {
                assert!(problems.iter().any(|p| p.contains("updated_specification")));
            }
            other => panic!("expected structural rejection, got {other:?}"),
        }
        assert_eq!(
            law_evidence(&dir),
            before,
            "no artifact or checkpoint may change"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_block_rejects_without_side_effects() {
        let (inputs, dir) = inputs_for("noblock", "go");
        let c = TurnController::start(
            inputs,
            Box::new(ScriptedHarness {
                canned: None,
                raw: Some("chatty but no json 😅".into()),
            }),
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
            document_updates: None,
            updated_specification: None,
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: Some(vec!["CLR-999".into()]),
            next_question_id: None,
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let c = TurnController::start(
            inputs,
            Box::new(ScriptedHarness {
                canned: Some(env),
                raw: None,
            }),
        );
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
            Box::new(ScriptedHarness {
                canned: None,
                raw: Some("{}\n".into()),
            }),
        );
        assert!(!c.cancel_requested());
        c.request_cancel();
        assert!(c.cancel_requested());
        let _ = drain(&c);
        let _ = std::fs::remove_dir_all(&dir);
    }
    struct StreamingHarness {
        gate: Arc<std::sync::Barrier>,
        text: String,
        fail: bool,
    }
    impl AiHarness for StreamingHarness {
        fn label(&self) -> String {
            "stream-fixture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("fixture".into())
        }
        fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            for text in ["# Draft", "# Draft\n\nLive content"] {
                req.progress_tx
                    .send(LiveProgress {
                        thoughts: "Reviewing the requested scope.".into(),
                        specification: Some(text.into()),
                        ..Default::default()
                    })
                    .unwrap();
            }
            self.gate.wait();
            if self.fail {
                return Err(AppError::Other("fixture failure".into()));
            }
            Ok(HarnessOutcome {
                final_text: self.text.clone(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }

    #[test]
    fn live_previews_precede_completion_and_never_write_unvalidated_content() {
        for mode in ["success", "invalid", "cancel", "failure"] {
            let (inputs, dir) = inputs_for(&format!("stream_{mode}"), "draft scope");
            let before = inputs.state.spec_text.clone();
            let gate = Arc::new(std::sync::Barrier::new(2));
            let text = if mode == "invalid" {
                "no envelope".into()
            } else {
                // Snake-case is the actual prompt contract, camelCase remains supported.
                serde_json::json!({"schema_version":1, "assistant_message":"Draft saved.",
                    "updated_specification":crate::core::specification::fixture("Live content"), "open_items_added":[],
                    "open_items_updated":[], "open_items_resolved":[]})
                .to_string()
            };
            let c = TurnController::start(
                inputs,
                Box::new(StreamingHarness {
                    gate: gate.clone(),
                    text,
                    fail: mode == "failure",
                }),
            );
            let mut previews = Vec::new();
            for _ in 0..2 {
                match c.poll(Duration::from_secs(3)) {
                    Some(TurnEvt::Progress(p)) => previews.push(p.specification.unwrap()),
                    _ => {
                        gate.wait();
                        panic!("expected live preview before completion");
                    }
                }
            }
            assert_eq!(previews, ["# Draft", "# Draft\n\nLive content"]);
            assert_eq!(PlannerState::load(&dir).unwrap().spec_text, before);
            if mode == "cancel" {
                c.request_cancel();
            }
            gate.wait();
            let outcome = drain(&c);
            match mode {
                "success" => assert!(matches!(outcome, TurnOutcome::Applied { .. })),
                "invalid" => assert!(matches!(outcome, TurnOutcome::Rejected { .. })),
                _ => assert!(matches!(outcome, TurnOutcome::HarnessFailed { .. })),
            }
            if mode != "success" {
                assert_eq!(PlannerState::load(&dir).unwrap().spec_text, before);
            }
            assert!(
                c.poll(Duration::ZERO).is_none(),
                "no stale preview may arrive after completion"
            );
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Byte-level pre/post evidence for zero-mutation proofs: the three
    /// planning artifacts plus the commit-chain length.
    fn law_evidence(root: &std::path::Path) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize) {
        let read = |rel: &str| {
            std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("reading {rel}: {e}"))
        };
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .expect("git rev-list ran");
        assert!(out.status.success(), "git rev-list failed");
        let count: usize = String::from_utf8(out.stdout)
            .unwrap()
            .trim()
            .parse()
            .expect("commit count parses");
        (
            read("planning/specification.md"),
            read("planning/open-items.md"),
            read(".planner/config.md"),
            count,
        )
    }

    fn git_stdout(root: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git ran");
        assert!(out.status.success(), "git {args:?} failed");
        String::from_utf8(out.stdout).unwrap()
    }

    /// Section 12(f) × the D-14 law, end-to-end on the git fixture: an
    /// envelope that misroutes `next_question_id` into a sole-owned lane
    /// REJECTS THE ENTIRE TURN with zero mutation (all three planning
    /// artifacts byte-identical, commit chain untouched); the equivalently
    /// shaped envelope aimed at the UNOWNED lane seat-inherits to the
    /// seated git-identified operator and lands as exactly one new
    /// imperative-subject checkpoint.
    #[test]
    fn routing_law_rejects_misroute_with_zero_mutation_and_applies_seat_inheritance() {
        let (mut inputs, dir) = inputs_for("routing-law", "Clarify the security and infosec lanes");

        // Shape the configuration: Security is SOLE-owned by Priya; InfoSec
        // is unowned — an entry EXISTS with an EMPTY member list, the exact
        // seeded-repo shape (must behave identically to a missing entry).
        {
            let mut st = inputs.state.clone();
            st.config.user = None;
            st.config.stakeholders = Stakeholders::new(vec![
                CategoryOwners::new("Security", vec!["Priya".into()]),
                CategoryOwners::new("InfoSec", Vec::new()),
            ]);
            std::fs::write(
                dir.join(".planner/config.md"),
                config_io::serialize(&st.config),
            )
            .unwrap();
            st.resync().unwrap();
            // Ticket 1's seat: the connected repo's git user (non-guest),
            // so seat inheritance can fire for this operator. "Packet Test"
            // stands in for the ticket's seated operator Zach: derived from
            // the connected repository's git config (not the config block or
            // (guest)), and a member of none of the lane-holder lists.
            assert_eq!(st.effective_user().name, "Packet Test");
            inputs.state = st;
        }

        // Seed one open Question item per lane through a scripted turn.
        let seed = TurnEnvelope {
            schema_version: Some(1),
            assistant_message: Some("Opened the lane-bound questions.".into()),
            change_summary: Some("raise security and infosec lane questions".into()),
            document_updates: None,
            updated_specification: Some(crate::core::specification::fixture(
                "Seeded for the routing-law demonstration.",
            )),
            open_items_added: Some(vec![
                TurnItem {
                    authority: None,
                    id: None,
                    kind: Some("Question".into()),
                    category: Some("Security".into()),
                    assigned_to: Some("Priya".into()),
                    priority: Some("Blocking".into()),
                    question: Some("How deep must the threat model go?".into()),
                    reason: Some("audit depth drives scope".into()),
                    resolution_note: None,
                    feature_id: None,
                    recommendation: None,
                    evidence: None,
                },
                TurnItem {
                    authority: None,
                    id: None,
                    kind: Some("Question".into()),
                    category: Some("InfoSec".into()),
                    assigned_to: Some("All".into()),
                    priority: Some("Blocking".into()),
                    question: Some("What is the incident-response cadence?".into()),
                    reason: Some("operational exposure".into()),
                    resolution_note: None,
                    feature_id: None,
                    recommendation: None,
                    evidence: None,
                },
            ]),
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: None,
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let ctrl = TurnController::start(
            inputs.clone(),
            Box::new(ScriptedHarness {
                canned: Some(seed),
                raw: None,
            }),
        );
        // Chain the applied state forward exactly like the app's tick loop:
        // each subsequent turn must start from the PREVIOUS outcome's state,
        // never from inputs' original snapshot.
        match drain(&ctrl) {
            TurnOutcome::Applied { state, .. } => inputs.state = state,
            other => panic!("seed turn should apply, got: {other:?}"),
        }
        // Sanity straight off disk: both lanes seeded with distinct ids.
        let seeded = PlannerState::load(&dir).unwrap();
        assert_eq!(seeded.items.len(), inputs.state.items.len());
        let sec_id = seeded
            .items
            .iter()
            .find(|i| i.category.eq_ignore_ascii_case("Security"))
            .expect("security item seeded")
            .id
            .clone();
        let info_id = seeded
            .items
            .iter()
            .find(|i| i.category.eq_ignore_ascii_case("InfoSec") && i.kind != ItemKind::Ownership)
            .expect("infosec question seeded")
            .id
            .clone();
        let before = law_evidence(&dir);
        assert!(before.3 >= 1, "the seed turn must have checkpointed");

        // MISROUTE: a valid spec replacement plus next_question_id pointing
        // at the SOLE-OWNED Security item → the whole turn is rejected.
        let misroute = TurnEnvelope {
            schema_version: Some(1),
            assistant_message: Some("Recorded the security decision; carrying on.".into()),
            change_summary: Some("record threat-model depth decision".into()),
            document_updates: None,
            updated_specification: Some(crate::core::specification::fixture(
                "Threat model settled to L2.",
            )),
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: Some(sec_id.clone()),
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let ctrl = TurnController::start(
            inputs.clone(),
            Box::new(ScriptedHarness {
                canned: Some(misroute),
                raw: None,
            }),
        );
        match drain(&ctrl) {
            TurnOutcome::Rejected { problems, .. } => {
                assert!(
                    problems
                        .iter()
                        .any(|p| p.contains(&sec_id) && p.contains("violates the routing law")),
                    "expected the routing-law fatal naming {sec_id}, got: {problems:?}"
                );
            }
            other => panic!("misrouted envelope must be Rejected, got: {other:?}"),
        }
        // Zero mutation: every planning artifact byte-identical, no new commit.
        assert_eq!(
            law_evidence(&dir),
            before,
            "the rejected turn must leave planning/specification.md, planning/open-items.md and .planner/config.md byte-identical and the commit chain untouched"
        );

        // PAIR: the equivalently shaped envelope proposes the UNOWNED
        // InfoSec item — seat inheritance makes it lawful for the seated
        // git-identified operator, and the turn applies with EXACTLY ONE
        // new imperative-subject checkpoint whose subject derives from
        // this change summary.
        const CADENCE_SUMMARY: &str = "record incident-response cadence";
        let inherited_spec =
            crate::core::specification::fixture("Incident cadence: page within the hour.");
        let inherited = TurnEnvelope {
            schema_version: Some(1),
            assistant_message: Some("Recorded the infosec decision; carrying on.".into()),
            change_summary: Some(CADENCE_SUMMARY.into()),
            document_updates: None,
            updated_specification: Some(inherited_spec.clone()),
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: Some(info_id.clone()),
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let ctrl = TurnController::start(
            inputs.clone(),
            Box::new(ScriptedHarness {
                canned: Some(inherited),
                raw: None,
            }),
        );
        match drain(&ctrl) {
            TurnOutcome::Applied {
                normalized,
                commit_result,
                state,
                ..
            } => {
                assert_eq!(
                    normalized.next_question_id.as_deref(),
                    Some(info_id.as_str()),
                    "the seat-inherited next question must be preserved"
                );
                assert!(commit_result.is_ok(), "commit failed: {commit_result:?}");
                // In-memory adoption: the returned state carries the turn's
                // specification change verbatim, and the posed question
                // remains in its queue.
                assert_eq!(
                    state.spec_text.as_deref(),
                    Some(inherited_spec.as_str()),
                    "applied in-memory state must adopt the turn's spec change"
                );
                assert!(
                    state
                        .items
                        .iter()
                        .any(|i| i.id == info_id && i.kind == ItemKind::Question),
                    "in-memory state must still carry the posed question"
                );
                inputs.state = state;
            }
            other => panic!("seat-inherited envelope must be Applied, got: {other:?}"),
        }
        let after = law_evidence(&dir);
        assert_eq!(
            after.3,
            before.3 + 1,
            "exactly one new checkpoint for the accepted turn"
        );
        assert_eq!(
            git_stdout(&dir, &["log", "-1", "--pretty=%s"]),
            format!("planner: {CADENCE_SUMMARY}\n"),
            "checkpoint subject must derive from the turn's imperative change summary"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
