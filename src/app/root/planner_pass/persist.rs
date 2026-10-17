pub(crate) fn persist(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    plan_revision: u64,
    spec_revision: &str,
    output: crate::harness::prompt_catalog::PlannerOutputV2,
) -> Result<crate::core::planning::ReadinessResult, String> {
    use crate::core::planning::{
        AcceptanceTestTrace, DEFAULT_NO_PROGRESS_THRESHOLD, QuestionCategory, QuestionOption,
        QuestionPriority, QuestionProposal, QuestionStatus, QuestionStore, ReadinessChecklistV1,
        propose_questions,
    };

    let questions = QuestionStore::new(store);
    let planning_revision = store.revision().map_err(|error| error.to_string())?;
    let required_requirement_ids =
        required_requirement_ids_for_snapshot(store, feature_uid, spec_revision)?;
    let mut current = questions.list().map_err(|error| error.to_string())?;
    let resolved = resolutions::apply(
        &questions,
        &mut current,
        feature_uid,
        plan_revision,
        output.resolved_questions,
    )?;
    let superseded = supersede_older_questions(&questions, &current, feature_uid, plan_revision)?;
    let proposals = output
        .proposed_questions
        .into_iter()
        .map(|question| QuestionProposal {
            feature_uid: feature_uid.to_owned(),
            plan_revision,
            decision_subject: question.decision_subject,
            decision_domain: question.decision_domain,
            origin_requirement_ids: question.origin_requirement_ids,
            category: match question.category {
                crate::harness::prompt_catalog::ClarificationCategory::Goal => {
                    QuestionCategory::Goal
                }
                crate::harness::prompt_catalog::ClarificationCategory::Scope => {
                    QuestionCategory::Scope
                }
                crate::harness::prompt_catalog::ClarificationCategory::Ux => QuestionCategory::Ux,
                crate::harness::prompt_catalog::ClarificationCategory::Integration => {
                    QuestionCategory::Integration
                }
                crate::harness::prompt_catalog::ClarificationCategory::Api => QuestionCategory::Api,
                crate::harness::prompt_catalog::ClarificationCategory::Data => {
                    QuestionCategory::Data
                }
                crate::harness::prompt_catalog::ClarificationCategory::Security => {
                    QuestionCategory::Security
                }
                crate::harness::prompt_catalog::ClarificationCategory::Privacy => {
                    QuestionCategory::Privacy
                }
                crate::harness::prompt_catalog::ClarificationCategory::Performance => {
                    QuestionCategory::Performance
                }
                crate::harness::prompt_catalog::ClarificationCategory::Operations => {
                    QuestionCategory::Operations
                }
                crate::harness::prompt_catalog::ClarificationCategory::Testing => {
                    QuestionCategory::Testing
                }
                crate::harness::prompt_catalog::ClarificationCategory::FailureMode => {
                    QuestionCategory::FailureMode
                }
                crate::harness::prompt_catalog::ClarificationCategory::Deployment => {
                    QuestionCategory::Deployment
                }
            },
            priority: match question.priority {
                crate::harness::prompt_catalog::ClarificationPriority::Blocking => {
                    QuestionPriority::Blocking
                }
                crate::harness::prompt_catalog::ClarificationPriority::Important => {
                    QuestionPriority::Important
                }
                crate::harness::prompt_catalog::ClarificationPriority::Optional => {
                    QuestionPriority::Optional
                }
            },
            question: question.question,
            reason: question.reason,
            options: question
                .options
                .into_iter()
                .map(|option| QuestionOption {
                    id: option.id,
                    label: option.label,
                    tradeoffs: option.tradeoffs,
                })
                .collect(),
            recommended_option_id: question.recommended_option_id,
            no_answer_impact: question.no_answer_impact,
            evidence_refs: question.evidence_refs,
            assumption_if_deferred: question.assumption_if_deferred,
            depends_on_question_ids: question.depends_on_question_ids,
        })
        .collect();
    let created = propose_questions(proposals, &current).map_err(|error| error.to_string())?;
    let mut all_questions = current
        .iter()
        .filter(|question| {
            question.feature_uid == feature_uid && question.plan_revision == plan_revision
        })
        .cloned()
        .collect::<Vec<_>>();
    for (question, _) in &superseded {
        if let Some(prior) = all_questions
            .iter_mut()
            .find(|prior| prior.uid == question.uid)
        {
            *prior = question.clone();
        }
    }
    all_questions.extend(created.iter().cloned());
    let existing = questions
        .load_readiness(feature_uid)
        .map_err(|error| error.to_string())?;
    let (record_revision, data_revision) = existing
        .as_ref()
        .map(|(checklist, record_revision)| (*record_revision, checklist.revision))
        .unwrap_or((0, 0));
    let mut checklist = ReadinessChecklistV1 {
        feature_uid: feature_uid.to_owned(),
        plan_revision,
        spec_revision: Some(spec_revision.to_owned()),
        required_requirement_ids: Some(required_requirement_ids.clone()),
        assessed_axes: output
            .readiness
            .assessed_axes
            .iter()
            .filter_map(|axis| parse_axis(axis))
            .collect(),
        acceptance_tests: output
            .readiness
            .acceptance_tests
            .into_iter()
            .map(|test| AcceptanceTestTrace {
                requirement_id: test.requirement_id,
                test_ref: test.test_ref,
            })
            .collect(),
        evidence_refs: output.readiness.evidence,
        unresolved_blocker_ids: all_questions
            .iter()
            .filter(|question| {
                question.priority == QuestionPriority::Blocking
                    && question.status == QuestionStatus::Open
                    && question.deferred.is_none()
            })
            .map(|question| question.uid.clone())
            .collect(),
        blocker_notes: output.readiness.blockers,
        ready: false,
        stalled: false,
        no_progress_count: existing
            .as_ref()
            .map_or(0, |(prior, _)| prior.no_progress_count),
        progress_fingerprint: existing
            .as_ref()
            .and_then(|(prior, _)| prior.progress_fingerprint.clone()),
        revision: data_revision,
    };
    if existing.is_some() {
        checklist.revision = data_revision
            .checked_add(1)
            .ok_or("readiness data revision exhausted")?;
    }
    crate::core::planning::observe_progress(
        &mut checklist,
        &all_questions,
        DEFAULT_NO_PROGRESS_THRESHOLD,
    );
    let assessment = crate::core::planning::assess_readiness(
        &all_questions,
        &checklist,
        &required_requirement_ids,
    );
    checklist.ready = matches!(&assessment, crate::core::planning::ReadinessResult::Ready);
    checklist.unresolved_blocker_ids = all_questions
        .iter()
        .filter(|question| {
            question.priority == QuestionPriority::Blocking
                && matches!(
                    question.status,
                    QuestionStatus::Draft | QuestionStatus::Open | QuestionStatus::Declined
                )
                && question.deferred.is_none()
        })
        .map(|question| question.uid.clone())
        .collect();
    let mut changed_questions = superseded;
    changed_questions.extend(resolved);
    changed_questions.extend(created.into_iter().map(|question| (question, 0)));
    questions
        .save_batch_with_readiness(
            &changed_questions,
            &checklist,
            record_revision,
            &planning_revision,
        )
        .map_err(|error| error.to_string())?;
    Ok(assessment)
}

mod helpers;
mod resolutions;
use helpers::{parse_axis, required_requirement_ids_for_snapshot, supersede_older_questions};

#[cfg(test)]
mod tests;
