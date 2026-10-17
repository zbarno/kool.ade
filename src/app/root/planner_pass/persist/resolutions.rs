use crate::core::planning::{
    EvidenceResolution, QuestionStatus, QuestionStore, QuestionV1, resolve_question_with_evidence,
};

pub(super) fn apply(
    store: &QuestionStore<'_>,
    current: &mut [QuestionV1],
    feature_uid: &str,
    plan_revision: u64,
    resolutions: Vec<crate::harness::prompt_catalog::TechnicalResolutionV1>,
) -> Result<Vec<(QuestionV1, u64)>, String> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let mut changed = Vec::new();
    for resolution in resolutions {
        let question = current
            .iter_mut()
            .find(|question| question.uid == resolution.question_uid)
            .ok_or_else(|| {
                "planner resolved a question outside the current question set".to_owned()
            })?;
        if question.feature_uid != feature_uid
            || question.plan_revision != plan_revision
            || question.status != QuestionStatus::Open
        {
            return Err("planner resolution targets a stale or non-open question".into());
        }
        let (mut persisted, record_revision) = store
            .load(&question.uid)
            .map_err(|error| error.to_string())?;
        if persisted != *question {
            return Err(
                "clarification changed while its evidence resolution was being applied".into(),
            );
        }
        resolve_question_with_evidence(
            &mut persisted,
            EvidenceResolution {
                expected_revision: question.revision,
                current_plan_revision: plan_revision,
                decision: &resolution.decision,
                rationale: &resolution.rationale,
                selected_option: resolution.selected_option_id.as_deref(),
                evidence_refs: &resolution.evidence_refs,
                now_ms,
            },
        )?;
        *question = persisted.clone();
        changed.push((persisted, record_revision));
    }
    Ok(changed)
}
