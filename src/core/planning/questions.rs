use super::QuestionV1;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionContent {
    pub uid: String,
    pub feature_uid: String,
    pub plan_revision: u64,
    pub question: String,
    pub reason: String,
    pub options: Vec<super::QuestionOption>,
    pub recommendation: Option<String>,
    pub no_answer_impact: String,
    pub evidence_refs: Vec<String>,
}

pub fn question_content(question: &QuestionV1) -> QuestionContent {
    QuestionContent {
        uid: question.uid.clone(),
        feature_uid: question.feature_uid.clone(),
        plan_revision: question.plan_revision,
        question: question.question.clone(),
        reason: question.reason.clone(),
        options: question.options.clone(),
        recommendation: question.recommended_option_id.clone(),
        no_answer_impact: question.no_answer_impact.clone(),
        evidence_refs: question.evidence_refs.clone(),
    }
}

pub(crate) fn render_markdown(question: &QuestionV1) -> Result<Vec<u8>, String> {
    question.validate()?;
    let mut output = format!(
        "# {}\n\n- UID: {}\n- Feature: {}\n- Plan revision: {}\n- Status: {:?}\n\n## Context\n\n{}\n\n## Why this decision matters\n\n{}\n\n## Options\n",
        question.question.trim(),
        question.uid,
        question.feature_uid,
        question.plan_revision,
        question.status,
        question.question.trim(),
        question.reason.trim()
    );
    for option in &question.options {
        output.push_str(&format!(
            "\n### {} — {}\n\n{}\n",
            option.id, option.label, option.tradeoffs
        ));
    }
    output.push_str(&format!(
        "\n## Recommendation\n\n{}\n\n## If unanswered\n\n{}\n\n## Evidence\n",
        question
            .recommended_option_id
            .as_deref()
            .unwrap_or("No recommendation"),
        question.no_answer_impact
    ));
    for evidence in &question.evidence_refs {
        output.push_str(&format!("\n- {evidence}"));
    }
    if let Some(deferred) = &question.deferred {
        output.push_str(&format!(
            "\n\n## Deferred assumption\n\n{}\n\n- Owner: {}\n- Risk: {}\n- Verification test: {}\n- Follow-up task: {}\n- Deferred by: {}\n- Deferred at (Unix ms): {}\n",
            escape_html(&deferred.assumption),
            escape_html(&deferred.owner),
            escape_html(&deferred.risk),
            escape_html(&deferred.verification_test),
            escape_html(&deferred.follow_up_task),
            escape_html(question.deferred_by.as_deref().unwrap_or("")),
            question.deferred_at_ms.unwrap_or_default()
        ));
    }
    let has_recorded_decision = matches!(
        question.status,
        super::QuestionStatus::Answered
            | super::QuestionStatus::EvidenceResolved
            | super::QuestionStatus::Declined
    ) || (question.status == super::QuestionStatus::Superseded
        && (question.answer_text.is_some()
            || question.selected_option.is_some()
            || question.answered_by.is_some()
            || question.answered_at_ms.is_some()));
    if has_recorded_decision {
        output.push_str("\n\n## Recorded decision\n\n");
        if question.status == super::QuestionStatus::Declined {
            output.push_str("The user declined to answer; no product decision was inferred.\n\n");
        } else if question.status == super::QuestionStatus::EvidenceResolved {
            output.push_str("Man.ager resolved this technical question using repository evidence; this is not a user decision.\n\n");
        } else if question.status == super::QuestionStatus::Superseded
            && question.answer_text.is_none()
            && question.selected_option.is_none()
        {
            output
                .push_str("A prior response was recorded before this question was superseded.\n\n");
        }
        if let Some(option_id) = question.selected_option.as_deref()
            && let Some(option) = question
                .options
                .iter()
                .find(|option| option.id == option_id)
        {
            output.push_str(&format!(
                "Selected **{} — {}**.\n\n",
                escape_html(option.id.as_str()),
                escape_html(option.label.as_str())
            ));
        }
        if let Some(answer) = question.answer_text.as_deref() {
            output.push_str("<pre>\n");
            output.push_str(&escape_html(answer));
            output.push_str("\n</pre>\n\n");
        }
        if let Some(actor) = question.answered_by.as_deref() {
            output.push_str(&format!("Answered by: {}\n\n", escape_html(actor)));
        }
        if let Some(answered_at_ms) = question.answered_at_ms {
            output.push_str(&format!("Answered at (Unix ms): {answered_at_ms}\n\n"));
        }
        output.push_str(&format!("Question revision: {}\n", question.revision));
    }
    output.push('\n');
    Ok(output.into_bytes())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
