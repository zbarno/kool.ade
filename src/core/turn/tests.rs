use super::*;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::execute::{EnvelopeDecode, decode_envelope};
use crate::core::prompt;
use crate::harness::{AiHarness, PlanningRequest, TurnEnvelope};

use crate::artifacts::config_io;
use crate::domain::{CategoryOwners, ItemKind, Stakeholders};
use crate::harness::{DocumentUpdate, HarnessOutcome, RetrievalPlan, TurnItem};

fn vision_update(text: &str) -> Vec<DocumentUpdate> {
    vec![DocumentUpdate {
        document_id: "product:overview".into(),
        content: format!("# Overview\n\n{text}\n"),
        status: None,
    }]
}

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
        assert!(req.system_instructions.contains(prompt::PLANNER_POLICY));
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

struct RetrievalPipelineHarness;

impl AiHarness for RetrievalPipelineHarness {
    fn label(&self) -> String {
        "retrieval-pipeline-test".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }

    fn plan_retrieval(&self, request: &PlanningRequest) -> Result<Option<RetrievalPlan>, AppError> {
        assert_eq!(
            request.mode,
            crate::harness::ExecutionMode::ReadOnlyAnalysis
        );
        assert!(
            request
                .system_instructions
                .contains("select authoritative context")
        );
        assert!(
            request
                .prompt_body
                .contains("The session vanishes after restart.")
        );
        Ok(Some(RetrievalPlan {
            documents: vec!["product:architecture-and-constraints".into()],
            open_items: vec![],
            repository_areas: vec!["src".into()],
        }))
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        assert_eq!(request.mode, crate::harness::ExecutionMode::Planning);
        assert!(request.system_instructions.contains(prompt::PLANNER_POLICY));
        assert!(
            request
                .prompt_body
                .contains("SESSION_CONTEXT_FROM_RETRIEVAL")
        );
        assert!(
            request
                .prompt_body
                .contains("Source: planning/product/architecture-and-constraints.md")
        );
        assert!(request.prompt_body.contains("repo:src/session.rs"));
        Ok(HarnessOutcome {
            final_text: serde_json::json!({
                "schema_version": 2,
                "assistant_message": "I will explain how session recovery works.",
                "change_summary": "explain session recovery"
            })
            .to_string(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn head_count(dir: &std::path::Path) -> usize {
    let out = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    if !out.status.success() {
        // Unborn HEAD: the fixture repos ship uncommitted at start.
        return 0;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// Acts as a rival writer: while the model is "running" it edits the spec
/// and checkpoints, exactly as another gated turn would.
struct RivalCheckpoint {
    raw: String,
    root: std::path::PathBuf,
    path: std::path::PathBuf,
}
impl AiHarness for RivalCheckpoint {
    fn label(&self) -> String {
        "rival-checkpoint".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, _req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let text = std::fs::read_to_string(&self.path).map_err(|e| AppError::Io {
            op: "read spec".into(),
            detail: e.to_string(),
        })?;
        std::fs::write(&self.path, format!("{text}\n\n<!-- rival writer -->\n")).map_err(|e| {
            AppError::Io {
                op: "edit spec".into(),
                detail: e.to_string(),
            }
        })?;
        for args in [
            ["add", "-A"].as_slice(),
            ["commit", "-qm", "rival: external edit"].as_slice(),
        ] {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .status()
                .map_err(|e| AppError::Io {
                    op: "git".into(),
                    detail: e.to_string(),
                })?
                .success();
            assert!(ok, "rival commit failed");
        }
        Ok(HarnessOutcome {
            final_text: self.raw.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn inputs_for(tag: &str, msg: &str) -> (TurnInputs, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("koolade_turn_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        ["init"].as_slice(),
        ["config", "user.email", "koolade@test.local"].as_slice(),
        ["config", "user.name", "Kool.ad/e Test"].as_slice(),
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
        comparison_feature: None,
    };
    let dir = inputs.state.repo_root.clone();
    (inputs, dir)
}

fn drain(controller: &TurnController) -> TurnOutcome {
    // `poll` returning None is an ordinary quiescent timeout while the
    // controller's keepalive sender holds the channel open, so it must not
    // be treated as a vanished turn. True vanishing is detected directly
    // via the worker handle; a bounded run of idle probes catches a
    // wedged worker (same shape as the persona-home probe budgets).
    const IDLE_BUDGET: usize = 8;
    let mut idle = 0usize;
    loop {
        match controller.poll(Duration::from_millis(250)) {
            Some(TurnEvt::Done(outcome)) => return *outcome,
            Some(TurnEvt::Progress(_)) => idle = 0,
            None => {
                idle += 1;
                if controller.worker.as_ref().is_some_and(|h| h.is_finished()) {
                    // The worker may have enqueued its terminal event
                    // moments before it exited; `Finished` proves the
                    // send completed, so drain once before deciding the
                    // turn truly vanished.
                    match controller.poll(Duration::ZERO) {
                        Some(TurnEvt::Done(outcome)) => return *outcome,
                        Some(TurnEvt::Progress(_)) => {}
                        None => {
                            panic!(
                                "turn vanished: worker exited without delivering a terminal outcome"
                            )
                        }
                    }
                } else {
                    assert!(
                        idle < IDLE_BUDGET,
                        "turn vanished: no terminal event within {IDLE_BUDGET} idle probes"
                    );
                }
            }
        }
    }
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

/// Byte-level pre/post evidence for zero-mutation proofs: the three
/// planning artifacts plus the commit-chain length.
fn law_evidence(root: &std::path::Path) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize) {
    let read =
        |rel: &str| std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("reading {rel}: {e}"));
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
        read(crate::artifacts::SPEC_FILE),
        read(crate::artifacts::OPEN_ITEMS_FILE),
        read(crate::artifacts::CONFIG_FILE),
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

/// CHG-003 capture harness: stores the exact `system_instructions` of
/// the one request it serves and answers with a canned Applied-envelope
/// reply, forcing the pipeline's real extraction path.
struct CaptureHarness {
    raw: String,
    sink: Arc<std::sync::Mutex<Option<String>>>,
}
impl AiHarness for CaptureHarness {
    fn label(&self) -> String {
        "capture-test".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("test".into())
    }
    fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        *self.sink.lock().unwrap() = Some(req.system_instructions.clone());
        Ok(HarnessOutcome {
            final_text: self.raw.clone(),
            envelope: None, // force the pipeline to extract + decode itself
            stderr_tail: String::new(),
        })
    }
}

/// Minimal envelope that validates cleanly in both turn modes.
fn canned_raw(message: &str) -> String {
    serde_json::json!({
        "schema_version": 1,
        "assistant_message": message,
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": []
    })
    .to_string()
}

mod application;
mod context;
mod conversations;
mod digest;
/// Hostile persona input must not alter machine-owned validation decisions.
mod persona_boundary;
/// Pipeline battery for operator persona binding and uncached reloads.
mod persona_injection;
mod routing;
mod validation;
