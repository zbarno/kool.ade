use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::core::context_build::TurnContext;
use crate::core::prompt;
use crate::core::state::PlannerState;
use crate::error::AppError;
use crate::harness::{AiHarness, LiveProgress, PlanningRequest};

use super::{TurnInputs, TurnOutcome};

pub(super) struct PreparedTurn {
    pub request: PlanningRequest,
    pub base_snapshot: Option<PlannerState>,
    pub user: crate::domain::CurrentUser,
}

pub(super) fn prepare(
    inputs: &TurnInputs,
    harness: &dyn AiHarness,
    cancel: &Arc<AtomicBool>,
    progress_tx: Sender<LiveProgress>,
    task: Option<&str>,
    timeout: Duration,
    started: Instant,
) -> Result<PreparedTurn, Box<TurnOutcome>> {
    // Snapshot the planning repo AS FOUND ON DISK (one retry: a single
    // transient read hiccup should not silently disable the guard). Caller-side
    // in-memory staging (e.g. an investigated item appended before its reply
    // turn) is not competitor motion — this turn's apply will persist it. A
    // rival writer (another turn, reconciliation, investigation, board
    // action) committing during the run IS drift, and a stale apply is refused.
    let base_snapshot =
        PlannerState::load_with_store(&inputs.state.repo_root, &inputs.state.planning_store)
            .or_else(|_| {
                PlannerState::load_with_store(&inputs.state.repo_root, &inputs.state.planning_store)
            })
            .ok();
    let user = inputs.state.effective_user();
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks
        && !inputs
            .state
            .workflow
            .ready(inputs.state.planning_contract())
    {
        return Err(Box::new(TurnOutcome::Rejected {
            problems: vec!["Continue the interview and approve task generation first.".into()],
            final_text: String::new(),
            elapsed: started.elapsed(),
        }));
    }
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks
        && let Some((id, _)) = &inputs.state.active_feature
        && !crate::core::workflow::feature_approved(
            &inputs.state.planning_store,
            &inputs.state.workflow,
            id,
        )
    {
        return Err(Box::new(TurnOutcome::Rejected {
            problems: vec![format!(
                "{id} needs explicit implementation approval before task generation"
            )],
            final_text: String::new(),
            elapsed: started.elapsed(),
        }));
    }
    if inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks {
        match PlannerState::load_with_store(&inputs.state.repo_root, &inputs.state.planning_store) {
            Ok(current)
                if current.spec_text == inputs.state.spec_text
                    && current.active_features == inputs.state.active_features
                    && inputs.state.active_feature.as_ref().is_none_or(|(id, body)| {
                        current.active_features.iter().any(|(current_id, current_body)| {
                            current_id == id && current_body == body
                        })
                    })
                    && current.repositories == inputs.state.repositories
                    && current.workflow == inputs.state.workflow
                    && current.items == inputs.state.items => {}
            _ => return Err(Box::new(TurnOutcome::Rejected { problems: vec!["Planning files changed since the readiness offer. Reopen the repository and review the current plan before generating tasks.".into()], final_text: String::new(), elapsed: started.elapsed() })),
        }
    }
    let comparison_state = if inputs.purpose == crate::core::workflow::TurnPurpose::ComparePlans {
        let Some(feature_id) = inputs.comparison_feature.as_deref() else {
            return Err(Box::new(TurnOutcome::Rejected {
                problems: vec!["Compare Plans requires a stable feature ID.".into()],
                final_text: String::new(),
                elapsed: started.elapsed(),
            }));
        };
        let Some((_, body)) = inputs
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
        else {
            return Err(Box::new(TurnOutcome::Rejected {
                problems: vec![format!(
                    "Compared feature {feature_id} is no longer active."
                )],
                final_text: String::new(),
                elapsed: started.elapsed(),
            }));
        };
        if !crate::domain::ChangeMetadata::require_markdown(body)
            .is_ok_and(|metadata| metadata.status == crate::domain::ChangeStatus::Ready)
        {
            return Err(Box::new(TurnOutcome::Rejected {
                problems: vec![format!("Compared feature {feature_id} is not Ready.")],
                final_text: String::new(),
                elapsed: started.elapsed(),
            }));
        }
        let mut state = inputs.state.clone();
        state.active_feature = Some((feature_id.to_owned(), body.clone()));
        Some(state)
    } else {
        None
    };
    let prompt_state = comparison_state.as_ref().unwrap_or(&inputs.state);
    let comparison_message = inputs
        .comparison_feature
        .as_deref()
        .map(|feature_id| format!("Compare plans for feature {feature_id}."));
    let prompt_message = comparison_message
        .as_deref()
        .unwrap_or(&inputs.user_message);
    let prompt_chat: &[(String, String)] =
        if inputs.purpose == crate::core::workflow::TurnPurpose::ComparePlans {
            &[]
        } else {
            &inputs.recent_chat
        };
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
    let task_note = task.and_then(|key| {
        if key.starts_with("planning:") || key.starts_with("feature:") {
            return None;
        }
        if let Some(work) = crate::core::planning_work::find(&inputs.state, key) {
            return Some(
                if work.kind == crate::core::planning_work::WorkKind::Question {
                    prompt::QUESTION_TASK_MODE_NOTE
                } else {
                    // Typed planning tasks have their own detailed body prompt and
                    // retain the standard interview contract. They are not a
                    // scoped item/document reply.
                    return None;
                },
            );
        }
        Some(prompt::TASK_CONVERSATION_MODE_NOTE)
    });
    let retrieval = if task.is_none() {
        match crate::core::context_retrieval::select(
            harness,
            prompt_state,
            prompt_message,
            prompt_chat,
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
        return Err(Box::new(TurnOutcome::HarnessFailed {
            error: AppError::HarnessFailed {
                reason: "cancelled by user".into(),
                stderr_tail: String::new(),
            },
            elapsed: started.elapsed(),
        }));
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
                return Err(Box::new(TurnOutcome::HarnessFailed {
                    error: AppError::Other(error),
                    elapsed: started.elapsed(),
                }));
            }
        }
    } else {
        let ctx = TurnContext::build_with_retrieval(
            prompt_state,
            prompt_message,
            prompt_chat,
            retrieval.as_ref(),
        );
        let mut body = prompt::render_prompt(&ctx);
        body.push_str(&prompt::workflow_context_for_turn(
            prompt_state,
            inputs.purpose,
            inputs.comparison_feature.as_deref(),
        ));
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
        task_id: None,
        reasoning_level: match inputs.purpose {
            crate::core::workflow::TurnPurpose::GenerateTasks => "off",
            _ => "xhigh",
        }
        .into(),
        telemetry_phase: None,
        repo_root: inputs.state.repo_root.clone(),
        runtime_config_source: None,
        prompt_body,
        system_instructions: prompt::compose_system_instructions(task_note, &persona_load.document),
        timeout,
        progress_tx,
        cancel: Arc::clone(cancel),
    };

    Ok(PreparedTurn {
        request,
        base_snapshot,
        user,
    })
}
