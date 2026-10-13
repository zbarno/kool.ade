use super::{QuestionPriority, QuestionProposal, QuestionStatus, QuestionV1};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuestionKey {
    pub subject: String,
    pub domain: String,
    pub requirements: Vec<String>,
}

pub fn normalize_decision_key(subject: &str, domain: &str, requirements: &[String]) -> QuestionKey {
    fn normalize(text: &str) -> String {
        let mut out = String::new();
        for c in text.chars().flat_map(char::to_lowercase) {
            if c.is_alphanumeric() {
                out.push(c);
            } else if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        }
        out.trim().to_owned()
    }
    let requirements = requirements
        .iter()
        .map(|s| normalize(s))
        .filter(|s| !s.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    QuestionKey {
        subject: normalize(subject),
        domain: normalize(domain),
        requirements,
    }
}

fn deterministic_uid(feature: &str, revision: u64, key: &QuestionKey) -> String {
    let mut hash = Sha256::new();
    hash.update(b"koolade-question-v1\0");
    hash.update(feature.as_bytes());
    hash.update(revision.to_be_bytes());
    hash.update(key.subject.as_bytes());
    hash.update([0]);
    hash.update(key.domain.as_bytes());
    for requirement in &key.requirements {
        hash.update([0]);
        hash.update(requirement.as_bytes());
    }
    let mut bytes: [u8; 16] = hash.finalize()[..16]
        .try_into()
        .expect("digest prefix length");
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}

pub fn propose_questions(
    mut proposals: Vec<QuestionProposal>,
    current: &[QuestionV1],
) -> Result<Vec<QuestionV1>, String> {
    // A stable ordering makes the chosen wording and duplicate option IDs
    // independent of provider response ordering.
    proposals.sort_by_cached_key(|proposal| serde_json::to_vec(proposal).unwrap_or_default());
    let mut grouped: BTreeMap<(String, u64, QuestionKey), QuestionProposal> = BTreeMap::new();
    for mut proposal in proposals {
        if proposal.question.trim().is_empty()
            || uuid::Uuid::parse_str(&proposal.feature_uid).is_err()
        {
            return Err("proposal requires a feature UUID and question".into());
        }
        proposal.origin_requirement_ids.sort();
        proposal.origin_requirement_ids.dedup();
        proposal.evidence_refs.sort();
        proposal.evidence_refs.dedup();
        let key = normalize_decision_key(
            &proposal.decision_subject,
            &proposal.decision_domain,
            &proposal.origin_requirement_ids,
        );
        let map_key = (proposal.feature_uid.clone(), proposal.plan_revision, key);
        grouped
            .entry(map_key)
            .and_modify(|existing| {
                existing
                    .origin_requirement_ids
                    .extend(proposal.origin_requirement_ids.clone());
                existing.origin_requirement_ids.sort();
                existing.origin_requirement_ids.dedup();
                existing
                    .evidence_refs
                    .extend(proposal.evidence_refs.clone());
                existing.evidence_refs.sort();
                existing.evidence_refs.dedup();
                existing
                    .depends_on_question_ids
                    .extend(proposal.depends_on_question_ids.clone());
                existing.depends_on_question_ids.sort();
                existing.depends_on_question_ids.dedup();
                if proposal.priority == QuestionPriority::Blocking {
                    existing.priority = QuestionPriority::Blocking;
                }
                existing.options.extend(proposal.options.clone());
                existing.options.sort_by(|a, b| a.id.cmp(&b.id));
                existing.options.dedup_by(|a, b| a.id == b.id);
            })
            .or_insert(proposal);
    }
    let existing_keys: BTreeSet<_> = current
        .iter()
        .filter(|q| q.status != QuestionStatus::Cancelled && q.status != QuestionStatus::Superseded)
        .map(|q| {
            (
                q.feature_uid.clone(),
                q.plan_revision,
                normalize_decision_key(
                    &q.decision_subject,
                    &q.decision_domain,
                    &q.origin_requirement_ids,
                ),
            )
        })
        .collect();
    let questions = grouped
        .into_iter()
        .filter_map(|((feature_uid, plan_revision, key), p)| {
            if existing_keys.contains(&(feature_uid.clone(), plan_revision, key.clone())) {
                return None;
            }
            let uid = deterministic_uid(&feature_uid, plan_revision, &key);
            Some(QuestionV1 {
                uid,
                feature_uid,
                plan_revision,
                decision_subject: p.decision_subject,
                decision_domain: p.decision_domain,
                origin_requirement_ids: p.origin_requirement_ids,
                category: p.category,
                priority: p.priority,
                question: p.question,
                reason: p.reason,
                options: p.options,
                recommended_option_id: p.recommended_option_id,
                no_answer_impact: p.no_answer_impact,
                evidence_refs: p.evidence_refs,
                assumption_if_deferred: p.assumption_if_deferred,
                deferred: None,
                depends_on_question_ids: p.depends_on_question_ids,
                status: QuestionStatus::Open,
                answer_text: None,
                selected_option: None,
                answered_by: None,
                answered_at_ms: None,
                revision: 0,
            })
        })
        .collect::<Vec<_>>();
    for question in &questions {
        question.validate()?;
    }
    Ok(questions)
}

pub fn reconcile_answer(
    question: &mut QuestionV1,
    expected_revision: u64,
    current_plan_revision: u64,
    answer: Option<&str>,
    selected: Option<&str>,
    answered_by: &str,
    now_ms: u64,
) -> Result<(), String> {
    if question.revision != expected_revision || question.plan_revision != current_plan_revision {
        return Err("stale answer: question or plan revision changed".into());
    }
    if question.status != QuestionStatus::Open {
        return Err("only open questions accept answers".into());
    }
    if let Some(option) = selected
        && !question
            .options
            .iter()
            .any(|candidate| candidate.id == option)
    {
        return Err("selected option does not exist".into());
    }
    if answer.is_none_or(|value| value.trim().is_empty()) && selected.is_none() {
        return Err("answer or explicit decline text is required".into());
    }
    if answered_by.trim().is_empty() || answered_by.len() > 256 || now_ms == 0 {
        return Err("answer attribution and timestamp are required".into());
    }
    if question.revision == u64::MAX {
        return Err("question revision is exhausted".into());
    }
    question.answer_text = answer
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    question.selected_option = selected.map(str::to_owned);
    question.answered_by = Some(answered_by.to_owned());
    question.answered_at_ms = Some(now_ms);
    question.status = QuestionStatus::Answered;
    question.revision += 1;
    question.validate()
}

/// Persist an explicit decline without treating it as a resolved product
/// decision. Required declined questions continue to block readiness.
pub fn decline_question(
    question: &mut QuestionV1,
    expected_revision: u64,
    current_plan_revision: u64,
    declined_by: &str,
    now_ms: u64,
) -> Result<(), String> {
    if question.revision != expected_revision || question.plan_revision != current_plan_revision {
        return Err("stale decline: question or plan revision changed".into());
    }
    if question.status != QuestionStatus::Open {
        return Err("only open questions accept a decline".into());
    }
    if declined_by.trim().is_empty() || declined_by.len() > 256 || now_ms == 0 {
        return Err("decline attribution and timestamp are required".into());
    }
    if question.revision == u64::MAX {
        return Err("question revision is exhausted".into());
    }
    question.status = QuestionStatus::Declined;
    question.answered_by = Some(declined_by.to_owned());
    question.answered_at_ms = Some(now_ms);
    question.revision += 1;
    question.validate()
}
