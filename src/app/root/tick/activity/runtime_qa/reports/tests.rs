use super::*;
use crate::{
    core::lifecycle::{TaskPhase, TaskRecord},
    runtime_qa::{QaFindingCategory, QaFindingSeverity},
};

fn task() -> TaskRecord {
    TaskRecord {
        schema_version: 1,
        task_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        feature_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        planning_item_uid: None,
        task_spec_sha256: None,
        parent_task_uid: None,
        subtask_uids: vec![],
        dependency_task_uids: vec![],
        phase: TaskPhase::AwaitingRuntimeQa,
        revision: 7,
        approved_plan_sha256: "a".repeat(64),
        implementation_sha256: Some("b".repeat(64)),
        reviewed_code_head_sha: Some("c".repeat(40)),
        reviewed_code_subject_sha256: Some("d".repeat(64)),
        code_review: None,
        review_history: vec![],
        repair_attempts: 0,
        runtime_qa: None,
        qa_execution: None,
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: None,
    }
}

fn request(task: &TaskRecord) -> QaRunRequestV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: task.task_uid.clone(),
        feature_uid: task.feature_uid.clone(),
        code_head_sha: "c".repeat(40),
        build_artifact_sha256: "e".repeat(64),
        complete_diff_hash: "b".repeat(64),
        review_subject_sha256: "d".repeat(64),
        approved_spec_hash: "a".repeat(64),
        requirement_ids: vec!["criterion-1".into()],
        acceptance_criteria: vec!["The result is correct".into()],
        relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
        startup_recipe_ref: "recipe:web".into(),
        startup_recipe_sha256: "f".repeat(64),
        execution_session_id: uid(),
        driver_profile_version: "web/v1".into(),
        test_data_fixture_ref: None,
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 8,
            max_actions_per_scenario: 16,
            max_total_actions: 64,
            timeout_seconds: 120,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 16,
        },
        author_run_id: uid(),
        author_session_id: uid(),
    }
}

fn finding(blocking: bool) -> crate::runtime_qa::QaFindingV1 {
    crate::runtime_qa::QaFindingV1 {
        finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        code_sha: "c".repeat(40),
        scenario_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_uids: vec!["criterion-1".into()],
        category: QaFindingCategory::Functional,
        severity: QaFindingSeverity::High,
        expected: "The result is correct".into(),
        actual: "The result is incorrect".into(),
        reproducible_steps: vec!["Submit the form".into()],
        screenshot_ids: vec![],
        trace_ids: vec![],
        smallest_fix: "Correct the result mapping".into(),
        blocking,
    }
}

#[test]
fn blocking_qa_finding_is_assigned_to_the_approved_task_repair() {
    let task = task();
    let request = request(&task);
    let finding = finding(true);
    let disposition = disposition_for_finding(&task, &request, &finding);

    assert_eq!(disposition.kind, QaFindingDispositionKind::RepairInScope);
    assert_eq!(disposition.status, QaFindingDispositionStatus::Assigned);
    assert_eq!(
        disposition.owner_item_uid.as_deref(),
        Some(task.task_uid.as_str())
    );
    assert_eq!(disposition.code_sha, request.code_head_sha);
    assert_eq!(disposition.impacted_requirements, finding.requirement_uids);
    assert!(disposition.reason.contains("Smallest fix"));
}

#[test]
fn nonblocking_qa_finding_is_persisted_as_a_user_decision() {
    let task = task();
    let request = request(&task);
    let finding = finding(false);
    let disposition = disposition_for_finding(&task, &request, &finding);

    assert_eq!(disposition.kind, QaFindingDispositionKind::AskUser);
    assert_eq!(
        disposition.status,
        QaFindingDispositionStatus::AwaitingAnswer
    );
    assert!(disposition.owner_item_uid.is_none());
    assert!(disposition.reason.contains("user's direction"));
    assert_eq!(disposition.expected_revision, task.revision);
}

#[test]
fn nonblocking_finding_creates_one_durable_clarification_on_replay() {
    use crate::{
        artifacts::planning_store::{PlanningStore, StoreMode},
        core::planning::{QuestionStatus, QuestionStore},
        runtime_qa::{QaCoverageV1, QaOutcome, QaResultV1},
    };

    let root = tempfile::tempdir().unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), root.path(), StoreMode::ManagedLocal);
    let task = task();
    let request = request(&task);
    let finding = finding(false);
    let disposition = disposition_for_finding(&task, &request, &finding);
    let result = QaResultV1 {
        schema_version: 1,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        outcome: QaOutcome::Fail,
        evidence_manifest_sha256: "f".repeat(64),
        cases: Vec::new(),
        coverage: QaCoverageV1 {
            requirements: Vec::new(),
            scenario_ids: Vec::new(),
            states: Vec::new(),
        },
        findings: vec![finding.clone()],
        tool_versions: std::collections::BTreeMap::new(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 1,
    };

    let mut decision_ids = None;
    for _ in 0..2 {
        let current = clarification::persist_user_decisions(
            &store,
            &task,
            &request,
            &result,
            std::slice::from_ref(&disposition),
        )
        .unwrap();
        if let Some(previous) = &decision_ids {
            assert_eq!(previous, &current);
        }
        decision_ids = Some(current);
    }

    let questions = QuestionStore::new(&store).list().unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].status, QuestionStatus::Open);
    assert_eq!(
        decision_ids.unwrap().get(&finding.finding_uid),
        Some(&questions[0].uid)
    );
    assert!(questions[0].decision_subject.contains(&finding.finding_uid));
    assert!(questions[0].reason.contains(&request.qa_run_id));
    assert!(
        questions[0]
            .evidence_refs
            .contains(&format!("runtime_qa_finding:{}", finding.finding_uid))
    );
}
