use super::*;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::execute::{EnvelopeDecode, decode_envelope};
use crate::core::prompt;
use crate::harness::{AiHarness, PlanningRequest, TurnEnvelope};

use crate::harness::{DocumentUpdate, HarnessOutcome, RetrievalPlan, TurnItem};

fn captured_persona(prompt: &str) -> Option<String> {
    let key = prompt.find("\"operator_persona\":")? + "\"operator_persona\":".len();
    let mut decoder = serde_json::Deserializer::from_str(prompt[key..].trim_start());
    <String as serde::Deserialize>::deserialize(&mut decoder).ok()
}

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
        if req.mode == crate::harness::ExecutionMode::TaskGeneration {
            assert!(req.system_instructions.contains(prompt::PLANNER_POLICY));
            assert!(req.prompt_manifest.is_none());
        } else {
            assert!(
                req.system_instructions
                    .contains("Planning Analyst for Kool.ad/e")
            );
            assert_eq!(
                req.prompt_manifest
                    .as_ref()
                    .map(|manifest| manifest.role_version.as_str()),
                Some("planner_conversation/v6")
            );
        }
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
    store: crate::artifacts::planning_store::PlanningStore,
}
impl AiHarness for RivalCheckpoint {
    fn label(&self) -> String {
        "rival-checkpoint".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, _req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let proposal = format!(
            "# Overview\n\nRival writer private proposal {}.\n",
            uuid::Uuid::new_v4()
        );
        let updates = vec![("product:overview".to_owned(), proposal)];
        let changes = crate::core::apply::private_document_draft_changes(&self.store, &updates)
            .map_err(|error| AppError::Io {
                op: "prepare private rival proposal".into(),
                detail: error.to_string(),
            })?;
        let revision = self.store.revision().map_err(|error| AppError::Io {
            op: "read private rival revision".into(),
            detail: error.to_string(),
        })?;
        let (_, committed_revision) = self
            .store
            .transaction_with_revision(&changes, Some(&revision))
            .map_err(|error| AppError::Io {
                op: "save private rival proposal".into(),
                detail: error.to_string(),
            })?;
        assert_ne!(
            committed_revision, revision,
            "rival proposal must advance store revision"
        );
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
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        ["init", "-b", "main"].as_slice(),
        ["config", "user.email", "koolade@test.local"].as_slice(),
        ["config", "user.name", "Kool.ad/e Test"].as_slice(),
    ] {
        let _ = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output();
    }
    seed_published_documentation(&root);
    let store = private_store(&root);
    let mut st = PlannerState::load_with_store(&root, &store).unwrap();
    st.bootstrap_missing().unwrap();
    // Seed only the private proposal workspace. Published documents remain
    // pinned from code Git; this local copy supplies a baseline for proposed
    // edits until they pass the publication gate.
    crate::artifacts::product_bootstrap::bootstrap_product_with_store(
        &root,
        &st.planning_store,
        "Fixture",
    )
    .unwrap();
    st = PlannerState::load_with_store(&root, &st.planning_store).unwrap();
    let inputs = TurnInputs {
        state: st,
        user_message: msg.into(),
        recent_chat: Vec::new(),
        purpose: crate::core::workflow::TurnPurpose::Interview,
        comparison_feature: None,
        specification_chat: None,
    };
    let dir = inputs.state.repo_root.clone();
    (inputs, dir)
}

fn private_store(root: &std::path::Path) -> crate::artifacts::planning_store::PlanningStore {
    let project_id = crate::core::state::test_support::project_id_for_code_root(root);
    crate::artifacts::planning_store::PlanningStore::for_code_repository(
        project_id,
        root.with_extension("private-state"),
        root,
    )
}

fn load_test_state(root: &std::path::Path) -> PlannerState {
    PlannerState::load_with_store(root, &private_store(root)).unwrap()
}

fn seed_published_documentation(root: &std::path::Path) {
    let docs = root.join("docs/koolade");
    let files = [
        (
            "AGENTS.md",
            "# Navigation\n\n- [architecture/](./architecture/AGENTS.md) - Architecture.\n- [features/](./features/AGENTS.md) - Features.\n- [product/](./product/AGENTS.md) - Product.\n",
        ),
        (
            "architecture/AGENTS.md",
            "# Navigation\n\n- [architecture-and-constraints.md](./architecture-and-constraints.md) - Architecture constraints.\n- [quality-and-acceptance.md](./quality-and-acceptance.md) - Quality requirements.\n",
        ),
        (
            "architecture/architecture-and-constraints.md",
            "# Architecture and Constraints\n\nTests use local application storage.\n",
        ),
        (
            "architecture/quality-and-acceptance.md",
            "# Quality and Acceptance\n\nChecks are recorded.\n",
        ),
        (
            "features/AGENTS.md",
            "# Navigation\n\n- [task-intents.md](./task-intents.md) - Private task intent model.\n",
        ),
        (
            "features/task-intents.md",
            "# Task intents\n\nTasks remain private local state.\n",
        ),
        (
            "product/AGENTS.md",
            "# Navigation\n\n- [overview.md](./overview.md) - Product overview.\n- [users-and-outcomes.md](./users-and-outcomes.md) - Users and outcomes.\n- [current-capabilities.md](./current-capabilities.md) - Capabilities.\n- [decisions.md](./decisions.md) - Decisions.\n- [quality-and-acceptance.md](./quality-and-acceptance.md) - Quality.\n- [architecture-and-constraints.md](./architecture-and-constraints.md) - Architecture.\n",
        ),
        (
            "product/overview.md",
            "# Overview\n\nKool.ad/e plans projects.\n",
        ),
        (
            "product/users-and-outcomes.md",
            "# Users and Outcomes\n\nOperators plan work.\n",
        ),
        (
            "product/current-capabilities.md",
            "# Current Capabilities\n\nPlanning conversations are supported.\n",
        ),
        (
            "product/decisions.md",
            "# Decisions\n\nNo fixture decisions.\n",
        ),
        (
            "product/quality-and-acceptance.md",
            "# Quality and Acceptance\n\nTests check outcomes.\n",
        ),
        (
            "product/architecture-and-constraints.md",
            "# Architecture and Constraints\n\nProject docs are published in code Git.\n",
        ),
    ];
    for (relative, content) in files {
        let path = docs.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    git_stdout(root, &["add", "docs/koolade"]);
    git_stdout(
        root,
        &["commit", "-qm", "publish synthetic test documentation"],
    );
}

fn drain(controller: &TurnController) -> TurnOutcome {
    // `poll` returning None is an ordinary quiescent timeout while the
    // controller's keepalive sender holds the channel open, so it must not
    // be treated as a vanished turn. True vanishing is detected directly
    // via the worker handle; a bounded run of idle probes catches a
    // wedged worker (same shape as the persona-home probe budgets).
    // A full parallel nextest run can starve fixture workers for tens of
    // seconds on a loaded host; retain a 60 second bound before declaring loss.
    const IDLE_BUDGET: usize = 240;
    let mut idle = 0usize;
    loop {
        match controller.poll(Duration::from_millis(250)) {
            Some(TurnEvt::Done(outcome)) => return *outcome,
            Some(TurnEvt::Progress(_)) => idle = 0,
            None => {
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
                    idle += 1;
                    assert!(idle < IDLE_BUDGET, "turn vanished: no terminal event");
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
    let store = private_store(root);
    let read = |rel: &str| {
        store
            .read(rel)
            .unwrap_or_else(|e| panic!("reading private {rel}: {e}"))
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
        git_stdout(root, &["show", "HEAD:docs/koolade/product/overview.md"]).into_bytes(),
        item_files_evidence(&store),
        read(crate::artifacts::planning_store::paths::PROJECT_CONFIG),
        count,
    )
}

fn item_files_evidence(store: &crate::artifacts::planning_store::PlanningStore) -> Vec<u8> {
    let mut files = Vec::new();
    for directory in [
        crate::artifacts::planning_store::paths::ITEM_CONTENTS,
        crate::artifacts::planning_store::paths::ITEM_STATES,
    ] {
        for entry in store.list_files(directory).unwrap() {
            let relative = format!("{directory}/{}", entry.name);
            files.push((relative.clone(), store.read(relative).unwrap()));
        }
    }
    for legacy in [
        crate::artifacts::planning_store::paths::OPEN_ITEMS,
        crate::artifacts::planning_store::paths::RESOLVED_ITEMS,
    ] {
        if let Ok(bytes) = store.read(legacy) {
            files.push((legacy.to_owned(), bytes));
        }
    }
    serde_json::to_vec(&files).unwrap()
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
        *self.sink.lock().unwrap() = Some(format!(
            "SYSTEM\n{}\nUSER\n{}",
            req.system_instructions, req.prompt_body
        ));
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
mod prompt_bounds;
mod validation;

mod read_only;
#[path = "tests/specification_chat.rs"]
mod specification_chat;
