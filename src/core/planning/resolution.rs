use super::{QuestionCategory, QuestionStatus, QuestionV1};

/// Record a repository-evidence decision for an implementation question. This
/// status is distinct from a user answer so technical precedent cannot be
/// mistaken for explicit product intent.
pub fn resolve_question_with_evidence(
    question: &mut QuestionV1,
    resolution: EvidenceResolution<'_>,
) -> Result<(), String> {
    let EvidenceResolution {
        expected_revision,
        current_plan_revision,
        decision,
        rationale,
        selected_option,
        evidence_refs,
        now_ms,
    } = resolution;
    if question.revision != expected_revision || question.plan_revision != current_plan_revision {
        return Err("stale evidence resolution: question or plan revision changed".into());
    }
    if question.status != QuestionStatus::Open {
        return Err("only open questions accept evidence resolutions".into());
    }
    if !matches!(
        question.category,
        QuestionCategory::Integration
            | QuestionCategory::Api
            | QuestionCategory::Performance
            | QuestionCategory::Operations
            | QuestionCategory::Testing
            | QuestionCategory::Deployment
    ) {
        return Err(
            "this question can change product intent and requires an explicit user answer".into(),
        );
    }
    if decision.trim().is_empty()
        || decision.len() > 4096
        || rationale.trim().is_empty()
        || rationale.len() > 8192
        || evidence_refs.is_empty()
        || evidence_refs.len() > 128
        || !evidence_refs
            .iter()
            .any(|reference| reference.starts_with("repository-overview:"))
        || now_ms == 0
        || selected_option
            .is_some_and(|option| !question.options.iter().any(|item| item.id == option))
    {
        return Err("evidence resolution is incomplete or invalid".into());
    }
    if question.revision == u64::MAX {
        return Err("question revision is exhausted".into());
    }
    question.answer_text = Some(format!(
        "Decision: {}\nRationale: {}",
        decision.trim(),
        rationale.trim()
    ));
    question.selected_option = selected_option.map(str::to_owned);
    question.answered_by = Some("Man.ager (repository evidence)".into());
    question.answered_at_ms = Some(now_ms);
    question.evidence_refs.extend(evidence_refs.iter().cloned());
    question.evidence_refs.sort();
    question.evidence_refs.dedup();
    question.status = QuestionStatus::EvidenceResolved;
    question.revision += 1;
    question.validate()
}

pub struct EvidenceResolution<'a> {
    pub expected_revision: u64,
    pub current_plan_revision: u64,
    pub decision: &'a str,
    pub rationale: &'a str,
    pub selected_option: Option<&'a str>,
    pub evidence_refs: &'a [String],
    pub now_ms: u64,
}

#[cfg(test)]
#[path = "resolution/tests.rs"]
mod tests;
