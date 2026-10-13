pub(crate) fn persist(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    plan_revision: u64,
    spec_revision: &str,
    output: crate::harness::prompt_catalog::PlannerOutputV2,
) -> Result<crate::core::planning::ReadinessResult, String> {
    use crate::core::planning::{
        AcceptanceTestTrace, QuestionCategory, QuestionOption, QuestionPriority, QuestionProposal,
        QuestionStatus, QuestionStore, ReadinessChecklistV1, propose_questions,
    };
    let questions = QuestionStore::new(store);
    let current = questions.list().map_err(|error| error.to_string())?;
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
    for question in &created {
        questions
            .save(question, 0)
            .map_err(|error| error.to_string())?;
    }
    let mut all_questions = questions.list().map_err(|error| error.to_string())?;
    all_questions.retain(|question| {
        question.feature_uid == feature_uid && question.plan_revision == plan_revision
    });
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
    crate::core::planning::observe_progress(&mut checklist, &all_questions, 3);
    questions
        .save_readiness(&checklist, record_revision)
        .map_err(|error| error.to_string())?;
    questions
        .reassess_readiness(feature_uid, plan_revision)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "readiness record disappeared after persistence".into())
}

fn parse_axis(value: &str) -> Option<crate::core::planning::ReadinessAxis> {
    use crate::core::planning::ReadinessAxis as Axis;
    Some(
        match value
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "")
            .as_str()
        {
            "who" => Axis::Who,
            "what" => Axis::What,
            "when" => Axis::When,
            "where" => Axis::Where,
            "why" => Axis::Why,
            "how" => Axis::How,
            "outofscope" => Axis::OutOfScope,
            "criticalpath" => Axis::CriticalPath,
            "systemboundaries" => Axis::SystemBoundaries,
            "failurebehavior" => Axis::FailureBehavior,
            "dataownership" => Axis::DataOwnership,
            "authorization" => Axis::Authorization,
            "measurablesuccess" => Axis::MeasurableSuccess,
            "testcases" => Axis::TestCases,
            _ => return None,
        },
    )
}

#[cfg(test)]
mod tests;
