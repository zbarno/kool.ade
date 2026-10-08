//! Autonomous resolution of one Agent-authority planning item. The worker
//! reads evidence through Pi, then uses the normal envelope validation and
//! transaction path to preserve a durable conclusion or escalate authority.
use crate::core::reconciliation::DEFER_PREFIX;
use crate::{
    core::{apply, context_build::TurnContext, gitops, prompt, state::PlannerState, validation},
    domain::Authority,
    harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest, TurnEnvelope},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

fn decode(text: &str) -> anyhow::Result<crate::harness::responses::InvestigationResponse> {
    crate::harness::responses::decode_investigation(text).map_err(anyhow::Error::msg)
}

fn validate_response(
    state: &PlannerState,
    item_id: &str,
    response: &crate::harness::responses::InvestigationResponse,
) -> anyhow::Result<validation::NormalizedTurn> {
    anyhow::ensure!(
        response.next_question_id.is_none(),
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
    anyhow::ensure!(
        response
            .open_items_added
            .as_deref()
            .unwrap_or_default()
            .iter()
            .all(|new| new.feature_id.is_some() && new.feature_id == item.feature_id),
        "New investigation findings must belong to the same feature"
    );
    let resolved = response.open_items_resolved.as_deref().unwrap_or_default();
    let updates = response.open_items_updated.as_deref().unwrap_or_default();
    let documents = response.document_updates.as_deref().unwrap_or_default();
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
    let normalized = validation::validate(
        &TurnEnvelope::from(response.clone()),
        state,
        &state.effective_user(),
    )
    .map_err(|problems| anyhow::anyhow!(problems.join("; ")))?;
    Ok(normalized)
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
        "{}\n=== AUTONOMOUS AGENT ITEM {} ===\nInvestigate this one item using read-only repository evidence. Do not ask the user. Use the supplied Kool.ad/e context first. Make no more than six read-only tool calls total; when reading files, request at most 200 lines, and when searching, specify a relevant path and a match limit of 20. Stop once the evidence resolves the item or shows that human judgment is required; if the evidence is insufficient within this budget, escalate instead of broadening the search. If the evidence or a safe reversible assumption resolves it, return document_updates with a full replacement of its related feature specification that records the finding and cites the source, and open_items_resolved=[\"{}\"]. Preserve other feature sections and approved intent. If it cannot safely be settled, return open_items_updated with only this item, authority Review or Human, and concrete evidence plus a provisional recommendation. Record newly discovered questions, assumptions and investigations in open_items_added with this same feature_id so they appear on the Kanban. Return exactly one JSON object with schema_version 2 and only these fields: schema_version, assistant_message, change_summary, document_updates, open_items_added, open_items_updated, open_items_resolved, next_question_id. Do not include requested_action, task stories, interview fields, or any other keys. Use the snake_case field names. For an escalation, match this shape: {{\"schema_version\":2,\"assistant_message\":\"brief finding\",\"document_updates\":[],\"open_items_added\":[],\"open_items_updated\":[{{\"id\":\"{}\",\"authority\":\"Human\",\"evidence\":\"source-backed facts\",\"recommendation\":\"provisional choice with reason\"}}],\"open_items_resolved\":[],\"next_question_id\":null}}. For a resolution, use the same outer keys, set open_items_updated to [], and include the required document_updates and open_items_resolved. All writes are application-validated.\n",
        prompt::render_prompt(&ctx),
        item.id,
        item.id,
        item.id
    );
    let mut correction = String::new();
    for attempt in 1..=3 {
        anyhow::ensure!(!cancel.load(Ordering::SeqCst), "Investigation cancelled");
        let request = PlanningRequest {
            mode: ExecutionMode::Investigation,
            task_id: None,
            reasoning_level: "off".into(),
            telemetry_phase: None,
            repo_root: state.repo_root.clone(),
            prompt_body: format!("{base}\n{correction}"),
            system_instructions: prompt::PLANNER_POLICY.into(),
            timeout: crate::core::turn::configured_turn_timeout(),
            progress_tx: progress.clone(),
            cancel: cancel.clone(),
        };
        let result = harness
            .execute(&request)
            .map_err(anyhow::Error::new)
            .and_then(|output| decode(&output.final_text))
            .and_then(|response| {
                validate_response(state, item_id, &response)
                    .map(|normalized| (TurnEnvelope::from(response), normalized))
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
                                    "{DEFER_PREFIX}: the project changed while the investigation ran; Kool.ad/e will retry shortly"
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
    Progress(Box<LiveProgress>),
    Done(Box<anyhow::Result<(PlannerState, String)>>),
}
pub struct Controller {
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    pub item_id: String,
    #[cfg(test)]
    _keepalive: Option<mpsc::Sender<Event>>,
}
impl Controller {
    pub fn start(state: PlannerState, item_id: String, harness: Box<dyn AiHarness>) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_id = item_id.clone();
        std::thread::spawn(move || {
            let (progress, updates) = mpsc::channel();
            let forward = tx.clone();
            let forwarder = std::thread::spawn(move || {
                for update in updates {
                    let _ = forward.send(Event::Progress(Box::new(update)));
                }
            });
            let result = run(
                &state,
                &worker_id,
                harness.as_ref(),
                progress,
                worker_cancel,
            );
            let _ = forwarder.join();
            let _ = tx.send(Event::Done(Box::new(result)));
        });
        Self {
            rx,
            cancel,
            item_id,
            #[cfg(test)]
            _keepalive: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn idle_fixture(item_id: impl Into<String>) -> Self {
        let (keepalive, rx) = mpsc::channel();
        Self {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            item_id: item_id.into(),
            _keepalive: Some(keepalive),
        }
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn cancellation_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
