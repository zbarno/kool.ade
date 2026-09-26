use super::{Brief, Report};
use std::collections::BTreeSet;

pub(super) fn validate(brief: &Brief, report: &Report) -> anyhow::Result<()> {
    anyhow::ensure!(
        (20..=1200).contains(&brief.problem.trim().chars().count()),
        "Explanation needs a concise, specific problem statement"
    );
    anyhow::ensure!(
        !brief.after.trim().is_empty() && brief.after.chars().count() <= 500,
        "Explanation needs a brief, concrete follow-up"
    );
    for step in &brief.steps {
        anyhow::ensure!(
            !step.owner.trim().is_empty()
                && !step.action.trim().is_empty()
                && step.action.chars().count() <= 500,
            "Explanation has an incomplete human step"
        );
    }
    let mut ids = BTreeSet::new();
    for option in &brief.options {
        anyhow::ensure!(
            !option.id.trim().is_empty()
                && ids.insert(option.id.trim().to_ascii_lowercase())
                && [
                    option.label.as_str(),
                    option.meaning.as_str(),
                    option.consequence.as_str()
                ]
                .iter()
                .all(|part| !part.trim().is_empty() && part.chars().count() <= 600),
            "Explanation has an incomplete or duplicate option"
        );
    }
    if !report.human_choices.is_empty() {
        let source_ids = report
            .human_choices
            .iter()
            .map(|choice| choice.id.trim().to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        anyhow::ensure!(
            ids == source_ids,
            "Explanation options do not match the report's choices"
        );
        for option in &brief.options {
            let choice = report
                .human_choices
                .iter()
                .find(|choice| choice.id.eq_ignore_ascii_case(option.id.trim()))
                .expect("validated choice ID has a saved source");
            anyhow::ensure!(
                option.source_evidence.as_deref() == Some(choice.label.as_str()),
                "Explanation choice is not grounded in its saved choice label"
            );
        }
    } else {
        anyhow::ensure!(
            brief.options.is_empty() || legacy_choices_are_grounded(brief, report),
            "Explanation choices must quote distinct alternatives from one saved human-action line"
        );
    }
    if let Some(recommendation) = &brief.recommendation {
        anyhow::ensure!(
            brief.options.iter().any(|option| {
                option
                    .id
                    .eq_ignore_ascii_case(recommendation.option_id.trim())
            }) && !recommendation.rationale.trim().is_empty()
                && recommendation.rationale.chars().count() <= 600,
            "Explanation has a recommendation that does not match a listed option"
        );
    }
    anyhow::ensure!(
        !brief.options.is_empty() || !brief.steps.is_empty(),
        "Explanation omits every human action"
    );
    Ok(())
}

/// Older reports may spell out alternatives in prose. The model may turn
/// those into buttons only when every button quotes a distinct phrase from
/// the same recorded human-action line; Rust does not guess English cues or
/// assume lettered IDs.
fn legacy_choices_are_grounded(brief: &Brief, report: &Report) -> bool {
    if brief.options.len() < 2 {
        return false;
    }
    let sources = brief
        .options
        .iter()
        .filter_map(|option| option.source_evidence.as_deref())
        .map(str::trim)
        .collect::<Vec<_>>();
    if sources.len() != brief.options.len()
        || sources.iter().any(|source| source.chars().count() < 4)
    {
        return false;
    }
    let unique = sources
        .iter()
        .map(|source| source.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    unique.len() == sources.len()
        && report
            .remaining
            .iter()
            .flat_map(|entry| entry.lines())
            .any(|line| sources.iter().all(|source| line.contains(source)))
}
