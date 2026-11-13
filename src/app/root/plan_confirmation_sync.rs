//! Durable synchronization of the taskintent-owned plan-confirmation card.
//!
//! Thin application seam around the pure-core reconciler: protect work that
//! holds an in-flight turn, run the core pass, persist changes, and return
//! user-visible side effects (copy for the user, pending evaluation
//! requests) to the caller. Card state itself flows exclusively through
//! `core::planning_work::reconcile_plan_confirmation`; this module never
//! infers state from chat text or card counts.

use crate::app::session::Project;
use crate::core::planning_work::ReadinessEvaluationRequest;

#[cfg(test)]
#[path = "plan_confirmation_sync/tests.rs"]
mod tests;

pub(crate) struct Effects {
    pub announcements: Vec<String>,
    pub evaluations_wanted: Vec<ReadinessEvaluationRequest>,
}

impl Effects {
    fn none() -> Self {
        Self {
            announcements: Vec::new(),
            evaluations_wanted: Vec::new(),
        }
    }
}

/// Busy work keys the reconciler must not mutate: anything holding an
/// in-flight turn (including a card-borne readiness evaluation).
fn busy_keys(project: &Project) -> std::collections::BTreeSet<String> {
    let mut busy: std::collections::BTreeSet<String> = project.task_turns.keys().cloned().collect();
    if let Some(key) = project.active_planning_work.clone() {
        busy.insert(key);
    }
    // Cancelled cards stay cancelled: the reconciler must never resurrect
    // operator-cancelled work.
    for work in &project.planning_work {
        if project
            .cancelled_work
            .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
        {
            busy.insert(work.key.clone());
        }
    }
    busy
}

#[cfg(test)]
pub(crate) fn sync(project: &mut Project) -> Effects {
    sync_with_prompt_claim(project, true)
}

/// Update the card and announce state changes without claiming the readiness
/// prompt. Used by result adoption; the periodic activity launcher claims it
/// when the single main-turn slot is available.
pub(crate) fn sync_for_announcement(project: &mut Project) -> Effects {
    sync_with_prompt_claim(project, false)
}

pub(crate) fn sync_with_prompt_claim(project: &mut Project, allow_prompt_claim: bool) -> Effects {
    sync_with_reconciler(
        project,
        allow_prompt_claim,
        |state, work, busy, allow_claim| {
            crate::core::planning_work::reconcile_with_prompt_claim(state, work, busy, allow_claim)
        },
    )
}

fn sync_with_reconciler(
    project: &mut Project,
    allow_prompt_claim: bool,
    reconcile: impl FnOnce(
        &crate::core::state::PlannerState,
        &mut Vec<crate::core::planning_work::Work>,
        &std::collections::BTreeSet<String>,
        bool,
    ) -> anyhow::Result<crate::core::planning_work::PlanConfirmationReport>,
) -> Effects {
    if project.activity.pending_planning_work
        && let Err(error) = project.save_planning_work()
    {
        return Effects {
            announcements: vec![format!(
                "Planning work is still waiting to be saved: {error}. Retrying shortly."
            )],
            evaluations_wanted: Vec::new(),
        };
    }
    let busy = busy_keys(project);
    let mut reconciled_work = project.planning_work.clone();
    let report = match reconcile(
        &project.state,
        &mut reconciled_work,
        &busy,
        allow_prompt_claim,
    ) {
        Ok(report) => {
            project.planning_work = reconciled_work;
            report
        }
        Err(_) => {
            // Fail closed: keep the existing card untouched and retry from
            // the next activity pass instead of making a silent no-op.
            project.activity.pending_planning_work = true;
            return Effects {
                announcements: vec![
                    "Couldn't refresh the plan review card from current evidence. The existing card is unchanged; Kool.ad/e will retry automatically.".into(),
                ],
                evaluations_wanted: Vec::new(),
            };
        }
    };
    if !report.changed {
        return Effects::none();
    }
    let mut effects = Effects {
        announcements: report
            .announcements
            .iter()
            .map(|notice| notice.excerpt.clone())
            .collect(),
        evaluations_wanted: report.evaluations_wanted,
    };
    if let Err(error) = project.save_planning_work() {
        // Retry machinery: mark dirty so the periodic reconcile retries.
        project.activity.pending_planning_work = true;
        // A failed save cannot consume a one-shot evaluation prompt. Keep the
        // claim unstamped so the next successful sync can persist it and emit
        // the request together.
        for evaluation in &effects.evaluations_wanted {
            if let Some(payload) = project
                .planning_work
                .iter_mut()
                .find(|work| work.key == evaluation.work_key)
                .and_then(|work| work.plan_confirmation.as_mut())
            {
                payload.evaluation_prompted_for = None;
            }
        }
        effects.announcements = vec![format!(
            "Couldn't save the plan review card: {error}. Retrying shortly."
        )];
        effects.evaluations_wanted.clear();
    }
    effects
}
