use crate::{
    artifacts::planning_store::PlanningStore,
    core::{
        lifecycle::TaskRecord,
        planning::{
            QuestionCategory, QuestionOption, QuestionPriority, QuestionProposal, QuestionStore,
            propose_questions,
        },
    },
    runtime_qa::{
        QaFindingDispositionKind, QaFindingDispositionStatus, QaFindingDispositionV1, QaResultV1,
        QaRunRequestV1,
    },
};
use std::collections::BTreeMap;

pub(super) fn persist_user_decisions(
    planning_store: &PlanningStore,
    task: &TaskRecord,
    request: &QaRunRequestV1,
    result: &QaResultV1,
    dispositions: &[QaFindingDispositionV1],
) -> Result<BTreeMap<String, String>, String> {
    let questions = QuestionStore::new(planning_store);
    let current = questions.list().map_err(|error| error.to_string())?;
    let plan_revision = questions
        .load_readiness(&task.feature_uid)
        .map_err(|error| error.to_string())?
        .map(|(readiness, _)| readiness.plan_revision)
        .or_else(|| {
            current
                .iter()
                .filter(|question| question.feature_uid == task.feature_uid)
                .map(|question| question.plan_revision)
                .max()
        })
        .unwrap_or(0);
    let mut decision_ids = BTreeMap::new();
    let proposals = result
        .findings
        .iter()
        .zip(dispositions)
        .filter(|(_, disposition)| {
            disposition.kind == QaFindingDispositionKind::AskUser
                && disposition.status == QaFindingDispositionStatus::AwaitingAnswer
        })
        .filter_map(|(finding, _)| {
            let subject = format!("Runtime QA finding {}", finding.finding_uid);
            if let Some(question) = current.iter().find(|question| {
                question.feature_uid == task.feature_uid
                    && question.plan_revision == plan_revision
                    && question.decision_domain == "runtime_qa_observation"
                    && question.decision_subject == subject
            }) {
                decision_ids.insert(finding.finding_uid.clone(), question.uid.clone());
                return None;
            }
            Some(QuestionProposal {
            feature_uid: task.feature_uid.clone(),
            plan_revision,
            decision_subject: subject,
            decision_domain: "runtime_qa_observation".into(),
            origin_requirement_ids: finding.requirement_uids.clone(),
            category: QuestionCategory::Testing,
            priority: QuestionPriority::Optional,
            question: format!(
                "How should Man.ager handle the nonblocking runtime QA finding {}?",
                finding.finding_uid
            ),
            reason: format!(
                "The approved behavior was {:?}, while QA observed {:?}. Suggested smallest fix: {}. QA run {} recorded reproducible steps: {}",
                finding.expected,
                finding.actual,
                finding.smallest_fix,
                request.qa_run_id,
                finding.reproducible_steps.join(" -> ")
            ),
            options: vec![
                QuestionOption {
                    id: "repair".into(),
                    label: "Plan a repair".into(),
                    tradeoffs: "Keeps the finding tied to its original approved requirement for Manager follow-up.".into(),
                },
                QuestionOption {
                    id: "accept-risk".into(),
                    label: "Accept the nonblocking risk".into(),
                    tradeoffs: "Keeps the finding visible as a documented product risk.".into(),
                },
            ],
            recommended_option_id: None,
            no_answer_impact: "The finding remains unresolved; it cannot be treated as accepted risk or closed by QA.".into(),
            evidence_refs: vec![
                format!("runtime_qa_run:{}", request.qa_run_id),
                format!("runtime_qa_finding:{}", finding.finding_uid),
                format!("task:{}", task.task_uid),
                format!("code_sha:{}", request.code_head_sha),
            ],
            assumption_if_deferred: None,
            depends_on_question_ids: Vec::new(),
        })
        })
        .collect::<Vec<_>>();
    for question in propose_questions(proposals, &current)? {
        let finding_uid = question
            .decision_subject
            .strip_prefix("Runtime QA finding ")
            .ok_or("runtime QA clarification subject is malformed")?;
        decision_ids.insert(finding_uid.to_owned(), question.uid.clone());
        match questions.save(&question, 0) {
            Ok(_) => {}
            Err(error) => {
                let existing = questions.load(&question.uid).map_err(|load_error| {
                    format!(
                        "{error}; could not confirm concurrent clarification creation: {load_error}"
                    )
                })?;
                if existing.0 != question {
                    return Err(format!(
                        "runtime QA clarification {} already exists with different content",
                        question.uid
                    ));
                }
            }
        }
    }
    Ok(decision_ids)
}
