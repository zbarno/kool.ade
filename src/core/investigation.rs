//! Autonomous resolution of one Agent-authority planning item. The worker
//! reads evidence through Pi, then uses the normal envelope validation and
//! transaction path to preserve a durable conclusion or escalate authority.
use crate::core::reconciliation::DEFER_PREFIX;
use crate::{
    core::{apply, context_build::TurnContext, gitops, prompt, state::PlannerState, validation},
    domain::Authority,
    harness::{AiHarness, LiveProgress, PiHarness, PlanningRequest, TurnEnvelope},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

fn decode(text: &str) -> anyhow::Result<TurnEnvelope> {
    let json = crate::harness::pi_extract::extract_json_object(text)
        .ok_or_else(|| anyhow::anyhow!("No complete investigation envelope"))?;
    Ok(serde_json::from_str(&json)?)
}

fn validate_response(
    state: &PlannerState,
    item_id: &str,
    envelope: &TurnEnvelope,
) -> anyhow::Result<validation::NormalizedTurn> {
    anyhow::ensure!(
        envelope.updated_specification.is_none()
            && envelope.interview.is_none()
            && envelope.task_stories.is_none()
            && envelope.task_outline.is_none()
            && envelope
                .open_items_added
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            && envelope.next_question_id.is_none(),
        "Investigation cannot alter unrelated workflow or ask the user"
    );
    let item = state
        .items
        .iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| anyhow::anyhow!("Investigation item no longer exists"))?;
    anyhow::ensure!(
        item.authority == Authority::Agent,
        "Investigation item is no longer Agent authority"
    );
    let resolved = envelope.open_items_resolved.as_deref().unwrap_or_default();
    let updates = envelope.open_items_updated.as_deref().unwrap_or_default();
    let documents = envelope.document_updates.as_deref().unwrap_or_default();
    anyhow::ensure!(
        resolved.iter().all(|id| id == item_id)
            && updates
                .iter()
                .all(|patch| patch.id.as_deref() == Some(item_id)),
        "Investigation addressed an unrelated item"
    );
    if resolved.contains(&item_id.to_string()) {
        anyhow::ensure!(updates.is_empty(), "Resolved item cannot also be updated");
        let feature_id = item
            .feature_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Agent resolution needs a related feature artifact"))?;
        anyhow::ensure!(
            documents.len() == 1 && documents[0].document_id == format!("feature:{feature_id}"),
            "Agent resolution must durably record the finding in its feature"
        );
    } else {
        anyhow::ensure!(
            documents.is_empty() && updates.len() == 1,
            "Unresolved investigation must escalate the one item without changing documents"
        );
        let update = &updates[0];
        anyhow::ensure!(
            matches!(
                update.authority.as_deref(),
                Some("Review" | "Human" | "review" | "human")
            ) && update
                .evidence
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                && update
                    .recommendation
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
            "Escalation requires evidence, a recommendation, and Review or Human authority"
        );
    }
    validation::validate(envelope, state, &state.effective_user())
        .map_err(|problems| anyhow::anyhow!(problems.join("; ")))
}

pub fn run(
    state: &PlannerState,
    item_id: &str,
    harness: &dyn AiHarness,
    progress: mpsc::Sender<LiveProgress>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<(PlannerState, String)> {
    run_with_settle_window(
        state,
        item_id,
        harness,
        progress,
        cancel,
        std::time::Duration::from_secs(5),
    )
}

fn run_with_settle_window(
    state: &PlannerState,
    item_id: &str,
    harness: &dyn AiHarness,
    progress: mpsc::Sender<LiveProgress>,
    cancel: Arc<AtomicBool>,
    settle: std::time::Duration,
) -> anyhow::Result<(PlannerState, String)> {
    let item = state
        .items
        .iter()
        .find(|item| item.id == item_id && item.authority == Authority::Agent)
        .ok_or_else(|| anyhow::anyhow!("Agent item is no longer open"))?;
    let request_text = format!(
        "Investigate {} using the existing specifications, decisions, tests and repository source. {}. {}",
        item.id, item.question, item.reason
    );
    let ctx = TurnContext::build(state, &request_text, &[]);
    let base = format!(
        "{}\n=== AUTONOMOUS AGENT ITEM {} ===\nInvestigate this one item using read-only repository evidence. Do not ask the user. If the evidence or a safe reversible assumption resolves it, return document_updates with a full replacement of its related feature specification that records the finding and cites the source, and open_items_resolved=[\"{}\"]. Preserve other feature sections and approved intent. If it cannot safely be settled, return open_items_updated with only this item, authority Review or Human, and concrete evidence plus a provisional recommendation. Return no other item changes, no question, no task stories, no interview. All writes are application-validated.\n",
        prompt::render_prompt(&ctx),
        item.id,
        item.id
    );
    let mut correction = String::new();
    for attempt in 1..=3 {
        anyhow::ensure!(!cancel.load(Ordering::SeqCst), "Investigation cancelled");
        let request = PlanningRequest {
            implementation: false,
            read_only: true,
            reasoning_level: "xhigh".into(),
            repo_root: state.repo_root.clone(),
            prompt_body: format!("{base}\n{correction}"),
            system_instructions: format!(
                "{}\n{}",
                prompt::SYSTEM_INSTRUCTIONS,
                prompt::SPECIFICATION_POLICY
            ),
            timeout: crate::core::turn::configured_turn_timeout(),
            progress_tx: progress.clone(),
            cancel: cancel.clone(),
        };
        let result = harness
            .execute(&request)
            .map_err(anyhow::Error::new)
            .and_then(|output| decode(&output.final_text))
            .and_then(|envelope| {
                validate_response(state, item_id, &envelope)
                    .map(|normalized| (envelope, normalized))
            });
        match result {
            Ok((envelope, normalized)) => {
                anyhow::ensure!(!cancel.load(Ordering::SeqCst), "Investigation cancelled");
                // Symmetric with reconciliation: give in-flight writers a short
                // settle window, then DEFER (benign, auto-retried by the UI)
                // rather than burn an alarming failure, and never write over a
                // newer commit.
                let deadline = std::time::Instant::now() + settle;
                loop {
                    anyhow::ensure!(!cancel.load(Ordering::SeqCst), "Investigation cancelled");
                    let guard = crate::core::writer_gate::acquire();
                    let attempt = (|| -> anyhow::Result<(PlannerState, String)> {
                        let current = PlannerState::load(&state.repo_root)?;
                        if !PlannerState::drift_report(state, &current).is_empty() {
                            anyhow::bail!("{DEFER_PREFIX}");
                        }
                        let mut next = state.clone();
                        let receipt = apply::apply(&mut next, &normalized)?;
                        let commit = gitops::commit(
                            &next.repo_root,
                            &receipt.commit_message,
                            &receipt.repo_relative_paths,
                        )
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "Investigation was saved but checkpoint failed: {error}"
                            )
                        })?;
                        Ok((next, format!("{} ({commit})", envelope.assistant())))
                    })();
                    drop(guard);
                    match attempt {
                        Ok(done) => return Ok(done),
                        Err(error) if error.to_string().starts_with(DEFER_PREFIX) => {
                            if std::time::Instant::now() >= deadline {
                                anyhow::bail!(
                                    "{DEFER_PREFIX}: the project changed while the investigation ran; Packet will retry shortly"
                                );
                            }
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            Err(error) => {
                correction = format!(
                    "REPAIR ATTEMPT {attempt}: {error}. Return a complete corrected schema_version 2 envelope.\n"
                );
                let _ = progress.send(LiveProgress {
                    activity: Some(correction.clone()),
                    ..Default::default()
                });
            }
        }
    }
    anyhow::bail!(
        "Agent item {item_id} could not be resolved or escalated after three attempts: {correction}"
    )
}

pub enum Event {
    Progress(LiveProgress),
    Done(anyhow::Result<(PlannerState, String)>),
}
pub struct Controller {
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    pub item_id: String,
}
impl Controller {
    pub fn start(state: PlannerState, item_id: String) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_id = item_id.clone();
        std::thread::spawn(move || {
            let (progress, updates) = mpsc::channel();
            let forward = tx.clone();
            let forwarder = std::thread::spawn(move || {
                for update in updates {
                    let _ = forward.send(Event::Progress(update));
                }
            });
            let result = run(&state, &worker_id, &PiHarness, progress, worker_cancel);
            let _ = forwarder.join();
            let _ = tx.send(Event::Done(result));
        });
        Self {
            rx,
            cancel,
            item_id,
        }
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{ItemKind, OpenItem, Priority},
        harness::HarnessOutcome,
    };
    use std::path::PathBuf;
    struct StaticHarness(String);
    impl AiHarness for StaticHarness {
        fn label(&self) -> String {
            "investigation fixture".into()
        }
        fn check_available(&self) -> Result<String, crate::error::AppError> {
            Ok("fixture".into())
        }
        fn execute(
            &self,
            request: &PlanningRequest,
        ) -> Result<HarnessOutcome, crate::error::AppError> {
            assert!(request.read_only && !request.implementation);
            assert!(
                request
                    .prompt_body
                    .contains("AUTONOMOUS AGENT ITEM CLR-001")
            );
            Ok(HarnessOutcome {
                final_text: self.0.clone(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }
    fn fixture() -> (PathBuf, PlannerState, String) {
        let root = std::env::temp_dir().join(format!(
            "packet_agent_item_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(root.join("planning/features/CHG-001-search")).unwrap();
        for args in [
            ["init", "-q"].as_slice(),
            ["config", "user.name", "Fixture"].as_slice(),
            ["config", "user.email", "fixture@example.test"].as_slice(),
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
        std::fs::write(root.join("planning/specification.md"), &legacy).unwrap();
        crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
        let feature = "# CHG-001: Search\n\n**Status:** Draft\n\n## Intent\n\nImprove search.\n\n## Current Behavior\n\nQuery persistence is unknown.\n\n## Desired Behavior\n\nQueries persist.\n\n## Scope\n\nSearch.\n\n## Affected Product Areas\n\n`product:05-functional-requirements`\n\n## Requirements\n\nQueries persist.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nRestart retains query.\n".to_string();
        std::fs::write(
            root.join("planning/features/CHG-001-search/specification.md"),
            &feature,
        )
        .unwrap();
        let mut item = OpenItem::new(
            "CLR-001".into(),
            Priority::Blocking,
            ItemKind::Ambiguity,
            "General".into(),
            Some("All".into()),
            "Does current query persistence survive restart?".into(),
            "Repository evidence may answer this.".into(),
        );
        item.authority = Authority::Agent;
        item.feature_id = Some("CHG-001".into());
        std::fs::write(
            root.join("planning/open-items.md"),
            crate::artifacts::items_io::serialize(&[item]),
        )
        .unwrap();
        let state = PlannerState::load(&root).unwrap();
        (root, state, feature)
    }
    #[test]
    fn evidence_resolves_agent_item_without_chat_question() {
        let (root, state, feature) = fixture();
        let revised = feature.replace("Query persistence is unknown.",
            "Query persistence is not implemented; src/search.rs only keeps the active query in memory.");
        let response = serde_json::json!({"schema_version":2,"assistant_message":"Verified current behavior from src/search.rs.",
            "document_updates":[{"document_id":"feature:CHG-001","content":revised}],
            "open_items_resolved":["CLR-001"],"next_question_id":null}).to_string();
        let (progress, _events) = mpsc::channel();
        let (updated, _) = run(
            &state,
            "CLR-001",
            &StaticHarness(response),
            progress,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(updated.items.is_empty());
        assert!(
            updated
                .active_feature
                .as_ref()
                .unwrap()
                .1
                .contains("src/search.rs")
        );
        assert!(
            crate::artifacts::items_io::parse(
                &std::fs::read_to_string(root.join("planning/open-items.md")).unwrap()
            )
            .unwrap()
            .is_empty()
        );
        let _ = std::fs::remove_dir_all(root);
    }
    /// Acts as a competing writer: edits the active feature document while
    /// the model is "running", as another gated writer would.
    struct DriftingPeer {
        raw: String,
        root: PathBuf,
    }
    impl AiHarness for DriftingPeer {
        fn label(&self) -> String {
            "drifting-peer".into()
        }
        fn check_available(&self) -> Result<String, crate::error::AppError> {
            Ok("fixture".into())
        }
        fn execute(
            &self,
            _req: &PlanningRequest,
        ) -> Result<HarnessOutcome, crate::error::AppError> {
            let path = self
                .root
                .join("planning/features/CHG-001-search/specification.md");
            let text = std::fs::read_to_string(&path)
                .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
            std::fs::write(
                &path,
                format!("{text}\nConcurrent edit by a gated writer.\n"),
            )
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
            Ok(HarnessOutcome {
                final_text: self.raw.clone(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }

    #[test]
    fn drifting_project_defers_instead_of_clobbering() {
        let (root, state, _) = fixture();
        let response = serde_json::json!({"schema_version":2,"assistant_message":"Cannot settle autonomously.",
            "document_updates":[],"open_items_updated":[{"id":"CLR-001","authority":"Review",
                "recommendation":"Defer.","evidence":"Insufficient evidence."}],"next_question_id":null}).to_string();
        let (progress, _events) = mpsc::channel();
        let error = run_with_settle_window(
            &state,
            "CLR-001",
            &DriftingPeer {
                raw: response,
                root: root.clone(),
            },
            progress,
            Arc::new(AtomicBool::new(false)),
            std::time::Duration::from_millis(200),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.starts_with(DEFER_PREFIX),
            "expected a benign deferral, got: {message}"
        );
        // The peer's edit survives; the investigation wrote nothing.
        let feature_now =
            std::fs::read_to_string(root.join("planning/features/CHG-001-search/specification.md"))
                .unwrap();
        assert!(feature_now.contains("Concurrent edit by a gated writer."));
        assert!(feature_now.contains("Query persistence is unknown."));
        let items = crate::artifacts::items_io::parse(
            &std::fs::read_to_string(root.join("planning/open-items.md")).unwrap(),
        )
        .unwrap();
        assert!(items.iter().any(|item| item.id == "CLR-001"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn uncertain_agent_item_becomes_review_with_evidence() {
        let (root, state, _) = fixture();
        let response = serde_json::json!({"schema_version":2,"assistant_message":"Repository evidence cannot establish retention policy.",
            "document_updates":[],"open_items_updated":[{"id":"CLR-001","authority":"Review",
                "recommendation":"Provisionally retain only the last ten queries.",
                "evidence":"src/search.rs has no persistence or retention contract."}],"next_question_id":null}).to_string();
        let (progress, _events) = mpsc::channel();
        let (updated, _) = run(
            &state,
            "CLR-001",
            &StaticHarness(response),
            progress,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(updated.items[0].authority, Authority::Review);
        assert!(!updated.items[0].recommendation.is_empty());
        assert!(!updated.items[0].evidence.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
