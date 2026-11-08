use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(super) enum QuestionCategory {
    Product,
    Code,
    Feature,
    Task,
    Opportunity,
    General,
}

pub(super) fn category(goal: &str) -> QuestionCategory {
    let value = goal.to_ascii_lowercase();
    if [
        "kool.ad/e",
        "manager",
        "capability",
        "feature catalog",
        "product",
    ]
    .iter()
    .any(|term| value.contains(term))
    {
        QuestionCategory::Product
    } else if [
        "architecture",
        "repository",
        "codebase",
        "source",
        "implemented",
        "build",
        "test",
    ]
    .iter()
    .any(|term| value.contains(term))
    {
        QuestionCategory::Code
    } else if [
        "feature",
        "specification",
        "requirement",
        "desired behavior",
    ]
    .iter()
    .any(|term| value.contains(term))
    {
        QuestionCategory::Feature
    } else if ["task", "worker", "review", "qa", "blocked", "progress"]
        .iter()
        .any(|term| value.contains(term))
    {
        QuestionCategory::Task
    } else if ["opportunity", "improvement", "suggest", "roadmap"]
        .iter()
        .any(|term| value.contains(term))
    {
        QuestionCategory::Opportunity
    } else {
        QuestionCategory::General
    }
}

pub(super) fn rank(id: &str, category: QuestionCategory) -> u8 {
    if id.starts_with("published-spec:")
        && matches!(
            category,
            QuestionCategory::Feature | QuestionCategory::Product
        )
    {
        return 0;
    }
    if id.starts_with("investigation:") {
        return 1;
    }
    if id.starts_with("code-review") || id.starts_with("runtime-qa") {
        return 2;
    }
    if id.starts_with("authorized-task-direction:") {
        return if matches!(category, QuestionCategory::Task) {
            1
        } else {
            3
        };
    }
    if id.starts_with("stale-authorized-task-direction:") {
        return if matches!(category, QuestionCategory::Task) {
            0
        } else {
            5
        };
    }
    if id.starts_with("published-spec:") {
        return 4;
    }
    match category {
        QuestionCategory::Product if id.contains("capability") => 1,
        QuestionCategory::Code if id.contains("repository") => 2,
        QuestionCategory::Feature if id.starts_with("plan-review:") => 2,
        QuestionCategory::Feature if id.starts_with("lifecycle-event:") => 3,
        QuestionCategory::Task if id.starts_with("plan-review:") => 1,
        QuestionCategory::Task if id.starts_with("operational-recovery:") => 2,
        QuestionCategory::Task if id == "workspace-summary" => 2,
        QuestionCategory::Opportunity if id.contains("capability") => 1,
        QuestionCategory::Opportunity if id.starts_with("task-failure") => 2,
        _ if id == "workspace-summary" => 3,
        _ => 5,
    }
}

pub(crate) fn terms(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.len() >= 4)
        .map(str::to_ascii_lowercase)
        .collect()
}

pub(crate) fn overlap(terms: &[String], candidate: &str) -> usize {
    let candidate = terms_for_candidate(candidate);
    terms
        .iter()
        .filter(|term| candidate.contains(term.as_str()))
        .count()
}

fn terms_for_candidate(text: &str) -> HashSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.len() >= 4)
        .map(str::to_ascii_lowercase)
        .collect()
}
