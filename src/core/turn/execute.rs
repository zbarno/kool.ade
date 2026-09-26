use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::core::apply::apply;
use crate::core::context_build::TurnContext;
use crate::core::gitops;
use crate::core::prompt;
use crate::core::state::PlannerState;
use crate::core::validation::{self};
use crate::error::AppError;
use crate::harness::{AiHarness, HarnessOutcome, LiveProgress, PlanningRequest, TurnEnvelope};

use super::{TurnInputs, TurnOutcome};

pub(super) fn run_turn(
    inputs: &TurnInputs,
    harness: &dyn AiHarness,
    cancel: &Arc<AtomicBool>,
    progress_tx: Sender<LiveProgress>,
    task: Option<&str>,
    timeout: Duration,
) -> TurnOutcome {
    let started = Instant::now();
    // Snapshot the planning repo AS FOUND ON DISK (one retry: a single
    // transient read hiccup should not silently disable the guard). Caller-side
    // in-memory staging (e.g. an investigated item appended before its reply
    // turn) is not competitor motion — this turn's apply will persist it. A
    // rival writer (another turn, reconciliation, investigation, board
    // action) committing during the run IS drift, and a stale apply is refused.
    let base_snapshot = PlannerState::load(&inputs.state.repo_root)
        .or_else(|_| PlannerState::load(&inputs.state.repo_root))
        .ok();
    let user = inputs.state.effective_user();
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
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks
        && let Some((id, _)) = &inputs.state.active_feature
        && !crate::core::workflow::feature_approved(
            &inputs.state.repo_root,
            &inputs.state.workflow,
            id,
        )
    {
        return TurnOutcome::Rejected {
            problems: vec![format!(
                "{id} needs explicit implementation approval before task generation"
            )],
            final_text: String::new(),
            elapsed: started.elapsed(),
        };
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
    // Operator persona layer (editable-operator-persona feature): load
    // the operator-owned document FRESH on every turn, deliberately
    // UNCACHED — a Settings save must bind from the very next turn with
    // no relaunch, the file is kilobyte-scale, and turns are
    // minute-scale. Sitting strictly after every pre-request Rejected
    // gate, gate-rejected turns perform no persona file IO and raise no
    // diagnostic; sitting strictly before request construction, the
    // composed instructions are what every conversation mode rides.
    let persona_load = crate::persistence::persona::load_persona();
    if let Some(diagnostic) = &persona_load.diagnostic {
        // Surface the store's own string on the progress channel the chat
        // pane already renders. Fire-and-forget: a dropped receiver means
        // the UI abandoned this turn, so the ignored Result keeps the
        // turn proceeding instead of sinking it.
        let _ = progress_tx.send(LiveProgress {
            activity: Some(format!("Persona note: {diagnostic}")),
            ..Default::default()
        });
    }
    let task_note = task
        .filter(|key| !key.starts_with("planning:") && !key.starts_with("feature:"))
        .map(|_| prompt::TASK_CONVERSATION_MODE_NOTE);
    let retrieval = if task.is_none() {
        match crate::core::context_retrieval::select(
            harness,
            &inputs.state,
            &inputs.user_message,
            &inputs.recent_chat,
            timeout.saturating_sub(started.elapsed()),
            progress_tx.clone(),
            Arc::clone(cancel),
        ) {
            Ok(selection) => selection,
            Err(error) => {
                let _ = progress_tx.send(LiveProgress {
                    activity: Some(format!(
                        "Context selection fell back to current project state: {}",
                        error.headline()
                    )),
                    ..Default::default()
                });
                None
            }
        }
    } else {
        None
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
    let mut prompt_body = if let Some(task) = task {
        match crate::core::task_conversation::prompt(
            &inputs.state,
            task,
            &inputs.user_message,
            &inputs.recent_chat,
        ) {
            Ok(body) => body,
            Err(error) => {
                return TurnOutcome::HarnessFailed {
                    error: AppError::Other(error),
                    elapsed: started.elapsed(),
                };
            }
        }
    } else {
        let ctx = TurnContext::build_with_retrieval(
            &inputs.state,
            &inputs.user_message,
            &inputs.recent_chat,
            retrieval.as_ref(),
        );
        let mut body = prompt::render_prompt(&ctx);
        body.push_str(&prompt::workflow_context(&inputs.state, inputs.purpose));
        body
    };
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks {
        prompt_body.push_str(prompt::TASK_OUTLINE_STEP);
    }
    let request = PlanningRequest {
        mode: if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks {
            crate::harness::ExecutionMode::TaskGeneration
        } else {
            crate::harness::ExecutionMode::Planning
        },
        reasoning_level: "xhigh".into(),
        repo_root: inputs.state.repo_root.clone(),
        prompt_body,
        system_instructions: prompt::compose_system_instructions(task_note, &persona_load.document),
        timeout,
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
    match decode_envelope(&outcome.final_text, inputs.purpose) {
        EnvelopeDecode::Env(env) => {
            if let Some(task) = task {
                let item_id = inputs.state.items.iter().find(|item| item.conversation_key() == task)
                    .map(|item| item.id.as_str()).unwrap_or(task);
                let feature_planning = task.starts_with("planning:") || task.starts_with("feature:");
                if env.requested_action.is_some()
                    || (!feature_planning && env.interview.is_some())
                    || env.task_stories.is_some()
                    || env.task_outline.is_some()
                    || (!feature_planning && env.next_question_id.as_deref().is_some_and(|id| id != item_id)) {
                    return TurnOutcome::Rejected {
                        problems: vec!["Task conversations cannot advance the project interview, generate tasks, or redirect the conversation to another item; they also cannot run project-level actions.".into()],
                        final_text: outcome.final_text, elapsed: started.elapsed(),
                    };
                }
            }
            // Validate against the PRE-mutation snapshot.
            match validation::validate_for_turn(&env, &inputs.state, &user, inputs.purpose) {
                Err(problems) => TurnOutcome::Rejected {
                    problems,
                    final_text: outcome.final_text,
                    elapsed: started.elapsed(),
                },
                Ok(normalized) => {
                    // Serialize with every other planning writer, then refuse
                    // to apply this snapshot if the project moved on disk in
                    // the meantime (concurrent turn, reconciliation,
                    // investigation, or board action). A stale apply would
                    // clobber the rival's newer commit; resending is cheaper.
                    let guard = crate::core::writer_gate::acquire();
                    let unchanged = PlannerState::load(&inputs.state.repo_root)
                        .is_ok_and(|current| match base_snapshot.as_ref() {
                            Some(base) => PlannerState::drift_report(base, &current).is_empty(),
                            None => true,
                        });
                    if !unchanged {
                        return TurnOutcome::Rejected {
                            problems: vec!["Planning files changed on disk while this turn was running, so nothing was saved. Review the latest artifacts and resend this message against the current state.".into()],
                            final_text: outcome.final_text, elapsed: started.elapsed(),
                        };
                    }
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
                        gitops::commit_cancellable(
                            &state.repo_root,
                            &receipt.commit_message,
                            &receipt.repo_relative_paths,
                            cancel,
                        )
                    };
                    drop(guard);
                    TurnOutcome::Applied {
                        state: Box::new(state),
                        receipt,
                        normalized: Box::new(normalized),
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

pub(super) enum EnvelopeDecode {
    Env(Box<TurnEnvelope>),
    Absent,
    Malformed(String),
}

pub(super) fn decode_envelope(
    final_text: &str,
    purpose: crate::core::workflow::TurnPurpose,
) -> EnvelopeDecode {
    use crate::harness::pi_extract::extract_json_object;
    match extract_json_object(final_text) {
        None => EnvelopeDecode::Absent,
        Some(blob) => match crate::harness::responses::decode_turn_object(&blob, purpose) {
            Ok(env) => EnvelopeDecode::Env(Box::new(env)),
            Err(e) => EnvelopeDecode::Malformed(e.to_string()),
        },
    }
}
