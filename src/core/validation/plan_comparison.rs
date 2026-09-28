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
    if !crate::domain::ChangeMetadata::require_markdown(body).is_ok_and(|metadata| {
        metadata.status == crate::domain::ChangeStatus::Ready && metadata.schema_version == 2
    }) {
        fatals.push("Compare Plans target must be a Ready feature".into());
    }
    let (Some(plans), Some(recommendation)) = (&envelope.plans, &envelope.recommendation) else {
        fatals.push("Compare Plans requires two alternatives and a recommendation".into());
        return None;
    };
    let comparison = PlanComparison {
        alternatives: plans.clone(),
        recommendation: recommendation.clone(),
        selected_plan: None,
    };
    if validate_persisted(&comparison).is_err() {
        fatals.push("Plan comparison is incomplete, malformed, or exceeds field limits".into());
        return None;
    }
    Some(comparison)
}

fn structurally_distinct(plans: &[crate::domain::PlanAlternative]) -> bool {
    if plans.len() != 2 {
        return false;
    }
    let a = &plans[0];
    let b = &plans[1];
    let phase_signature = |plan: &crate::domain::PlanAlternative| {
        plan.phases
            .iter()
            .map(|phase| {
                std::iter::once(phase.name.as_str())
                    .chain(phase.subtasks.iter().map(String::as_str))
                    .map(normalize_words)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    phase_signature(a) != phase_signature(b)
        || sorted_paths(&a.files_touched) != sorted_paths(&b.files_touched)
        || normalized_list(&a.state_changes) != normalized_list(&b.state_changes)
        || normalized_list(&a.failure_modes) != normalized_list(&b.failure_modes)
        || normalized_list(&a.known_risks) != normalized_list(&b.known_risks)
        || normalize_words(&a.reversibility) != normalize_words(&b.reversibility)
}

pub(crate) fn validate_persisted(comparison: &PlanComparison) -> anyhow::Result<()> {
    let valid_text =
        |value: &str, cap: usize| !value.trim().is_empty() && value.chars().count() <= cap;
    let plans = &comparison.alternatives;
    let recommendation = &comparison.recommendation;
    let valid = structurally_distinct(plans)
        && plans.len() == 2
        && plans[0].id == "A"
        && plans[1].id == "B"
        && ["A", "B"].contains(&recommendation.plan_id.as_str())
        && valid_text(&recommendation.rationale, 1200)
        && !recommendation.evidence.is_empty()
        && recommendation
            .evidence
            .iter()
            .all(|item| valid_text(item, 500))
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
                && plan.phases.iter().all(|phase| {
                    valid_text(&phase.name, 160)
                        && (1..=3).contains(&phase.subtasks.len())
                        && phase.subtasks.iter().all(|task| valid_text(task, 300))
                })
                && plan.files_touched.iter().all(|item| valid_text(item, 300))
                && plan.state_changes.iter().all(|item| valid_text(item, 500))
                && plan.failure_modes.iter().all(|item| valid_text(item, 500))
                && plan.known_risks.iter().all(|item| valid_text(item, 500))
        })
        && comparison
            .selected_plan
            .as_deref()
            .is_none_or(|selected| ["A", "B"].contains(&selected));
    anyhow::ensure!(valid, "Plan comparison record is incomplete or malformed");
    Ok(())
}

fn sorted_paths(paths: &[String]) -> Vec<&str> {
    let mut sorted = paths.iter().map(String::as_str).collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted
}

fn normalized_list(values: &[String]) -> Vec<Vec<String>> {
    values.iter().map(|value| normalize_words(value)).collect()
}

fn normalize_words(value: &str) -> Vec<String> {
    const GENERIC: &[&str] = &[
        "a",
        "an",
        "and",
        "the",
        "to",
        "for",
        "with",
        "use",
        "using",
        "add",
        "create",
        "build",
        "implement",
        "update",
        "move",
        "change",
        "handle",
        "provide",
        "ensure",
        "support",
        "then",
        "before",
        "after",
    ];
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|word| !word.is_empty() && !GENERIC.contains(&word.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(id: &str, path: &str, phase: &str, subtask: &str) -> crate::domain::PlanAlternative {
        crate::domain::PlanAlternative {
            id: id.into(),
            objective: "Maintain a safe rollout".into(),
            phases: vec![
                crate::domain::PlanPhase {
                    name: phase.into(),
                    subtasks: vec![subtask.into()]
                };
                3
            ],
            files_touched: vec![path.into()],
            state_changes: vec!["Keep the feature state".into()],
            failure_modes: vec!["A write fails".into()],
            effort_band: "Small — one file".into(),
            known_risks: vec!["One extra read".into()],
            reversibility: "Remove the state marker".into(),
        }
    }

    #[test]
    fn contrast_guard_rejects_wording_only_changes_but_accepts_real_boundaries() {
        let a = plan("A", "src/store.rs", "Add record guard", "Use record guard");
        let wording_only = plan(
            "B",
            "src/store.rs",
            "Create record guard",
            "Implement record guard",
        );
        assert!(!structurally_distinct(&[a.clone(), wording_only]));
        let different = plan(
            "B",
            "src/index.rs",
            "Build reverse index",
            "Store keys by account",
        );
        assert!(structurally_distinct(&[a, different]));
    }
}
