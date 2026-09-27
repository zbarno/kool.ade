use crate::{core::state::PlannerState, domain::PlanComparison, harness::TurnEnvelope};

pub(super) fn validate(
    envelope: &TurnEnvelope,
    state: &PlannerState,
    fatals: &mut Vec<String>,
) -> Option<PlanComparison> {
    if envelope
        .document_updates
        .as_ref()
        .is_some_and(|v| !v.is_empty())
        || envelope.updated_specification.is_some()
        || envelope
            .open_items_added
            .as_ref()
            .is_some_and(|v| !v.is_empty())
        || envelope
            .open_items_updated
            .as_ref()
            .is_some_and(|v| !v.is_empty())
        || envelope
            .open_items_resolved
            .as_ref()
            .is_some_and(|v| !v.is_empty())
        || envelope.requested_action.is_some()
        || envelope.interview.is_some()
        || envelope.task_stories.is_some()
        || envelope.task_outline.is_some()
    {
        fatals.push("Compare Plans may only return alternatives and a recommendation".into());
    }
    let Some((_, body)) = state.active_feature.as_ref() else {
        fatals.push("Compare Plans has no active feature target".into());
        return None;
    };
    if !crate::domain::ChangeMetadata::require_markdown(body)
        .is_ok_and(|metadata| metadata.status == crate::domain::ChangeStatus::Ready)
    {
        fatals.push("Compare Plans target must be a Ready feature".into());
    }
    let (Some(plans), Some(recommendation)) = (&envelope.plans, &envelope.recommendation) else {
        fatals.push("Compare Plans requires two alternatives and a recommendation".into());
        return None;
    };
    let valid_text =
        |value: &str, cap: usize| !value.trim().is_empty() && value.chars().count() <= cap;
    let structurally_distinct = plans.len() == 2
        && (plans[0].phases != plans[1].phases
            || plans[0].files_touched != plans[1].files_touched
            || plans[0].state_changes != plans[1].state_changes
            || plans[0].failure_modes != plans[1].failure_modes
            || plans[0].reversibility != plans[1].reversibility);
    let valid = structurally_distinct
        && plans.len() == 2
        && plans[0].id == "A"
        && plans[1].id == "B"
        && ["A", "B"].contains(&recommendation.plan_id.as_str())
        && valid_text(&recommendation.rationale, 1200)
        && !recommendation.evidence.is_empty()
        && recommendation.evidence.iter().all(|v| valid_text(v, 500))
        && plans.iter().all(|plan| {
            valid_text(&plan.objective, 600)
                && plan.phases.len() == 3
                && !plan.files_touched.is_empty()
                && plan.files_touched.len() <= 20
                && !plan.state_changes.is_empty()
                && plan.state_changes.len() <= 12
                && !plan.failure_modes.is_empty()
                && plan.failure_modes.len() <= 12
                && valid_text(&plan.effort_band, 300)
                && ["small", "medium", "large"]
                    .iter()
                    .any(|band| plan.effort_band.to_ascii_lowercase().starts_with(band))
                && !plan.known_risks.is_empty()
                && plan.known_risks.len() <= 12
                && valid_text(&plan.reversibility, 500)
                && plan.phases.iter().all(|v| valid_text(v, 500))
                && plan.files_touched.iter().all(|v| valid_text(v, 300))
                && plan.state_changes.iter().all(|v| valid_text(v, 500))
                && plan.failure_modes.iter().all(|v| valid_text(v, 500))
                && plan.known_risks.iter().all(|v| valid_text(v, 500))
        });
    if !valid {
        fatals.push("Plan comparison is incomplete, malformed, or exceeds field limits".into());
        return None;
    }
    Some(PlanComparison {
        alternatives: plans.clone(),
        recommendation: recommendation.clone(),
        selected_plan: None,
    })
}
