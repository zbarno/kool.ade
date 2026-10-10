mod decode;
mod generation;
mod request;
pub(super) use decode::{EnvelopeDecode, decode_envelope};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::core::apply::apply;
use crate::core::gitops;
use crate::core::state::PlannerState;
use crate::core::validation::{self};
use crate::error::AppError;
use crate::harness::{AiHarness, LiveProgress};

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
    let prepared =
        match request::prepare(inputs, harness, cancel, progress_tx, task, timeout, started) {
            Ok(prepared) => prepared,
            Err(outcome) => return *outcome,
        };
    let base_snapshot = prepared.base_snapshot;
    let user = prepared.user;
    let request = prepared.request;
    let result = generation::execute(
        inputs.purpose == crate::core::workflow::TurnPurpose::GenerateTasks,
        harness,
        &request,
        &inputs.state,
        started,
    );
    let (outcome, generation_revision) = match result {
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
            if inputs.purpose == crate::core::workflow::TurnPurpose::Question
                && (env.updated_specification.is_some()
                    || env.document_updates.as_ref().is_some_and(|items| !items.is_empty())
                    || env.interview.is_some()
                    || env.task_stories.is_some()
                    || env.planning_tasks.as_ref().is_some_and(|items| !items.is_empty())
                    || env.task_outline.is_some()
                    || env.requested_action.is_some()
                    || env.plans.is_some()
                    || env.recommendation.is_some())
            {
                return TurnOutcome::Rejected {
                    problems: vec!["Question tasks cannot create or update specifications, task stories, or project actions. Answer the question directly; raise only a related unresolved board item when needed.".into()],
                    final_text: outcome.final_text,
                    elapsed: started.elapsed(),
                };
            }
            let documentation_refresh = task
                .and_then(|key| crate::core::planning_work::find(&inputs.state, key))
                .is_some_and(|work| {
                    work.kind == crate::core::planning_work::WorkKind::DocumentationRefresh
                });
            if env
                .planning_tasks
                .as_ref()
                .is_some_and(|items| !items.is_empty())
                && !documentation_refresh
            {
                return TurnOutcome::Rejected {
                    problems: vec![
                        "Only Refresh Documentation tasks can create discovery tasks.".into(),
                    ],
                    final_text: outcome.final_text,
                    elapsed: started.elapsed(),
                };
            }
            if let Some(task) = task {
                let item_id = inputs.state.items.iter().find(|item| item.conversation_key() == task)
                    .map(|item| item.id.as_str()).unwrap_or(task);
                let feature_planning = task.starts_with("planning:")
                    || task.starts_with("feature:")
                    || crate::core::planning_work::find(&inputs.state, task)
                        .is_some_and(|work| work.kind != crate::core::planning_work::WorkKind::Question);
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
            let user_replied_item_ids =
                super::controller::user_replied_human_item_ids(&inputs.state, task);
            match validation::validate_for_turn_with_resolutions(
                &env,
                &inputs.state,
                &user,
                inputs.purpose,
                &user_replied_item_ids,
            ) {
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
                    let unchanged = PlannerState::load_with_store(
                        &inputs.state.repo_root,
                        &inputs.state.planning_store,
                    )
                        .is_ok_and(|current| {
                            generation::state_matches_revision(
                                &current,
                                base_snapshot.as_ref(),
                                generation_revision.as_deref(),
                            )
                        });
                    if !unchanged {
                        return TurnOutcome::Rejected {
                            problems: vec!["Planning files changed on disk while this turn was running, so nothing was saved. Review the latest artifacts and resend this message against the current state.".into()],
                            final_text: outcome.final_text, elapsed: started.elapsed(),
                        };
                    }
                    let mut state = inputs.state.clone();
                    generation::adopt_revision(&mut state, generation_revision);
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
                            &state.planning_store.git_root(),
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
