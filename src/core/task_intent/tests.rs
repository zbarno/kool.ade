use super::*;

#[path = "tests/done_gates.rs"]
mod done_gates;

fn root(kind: WorkKind, status: WorkStatus) -> Work {
    let mut work = Work::new(
        "task:request".into(),
        "Original request".into(),
        "Keep this goal through delivery".into(),
        "Current planning detail".into(),
    );
    work.kind = kind;
    work.status = status;
    work.task_intent_uid = Some(work.uid.clone());
    work
}

#[test]
fn planning_requests_complete_without_irrelevant_pull_request_gates() {
    let question = root(WorkKind::Question, WorkStatus::Done);
    let intent = TaskIntent::project(&question, &[], None, &[], None).unwrap();
    assert_eq!(intent.uid, question.uid);
    assert_eq!(intent.original_goal, question.request);
    assert_eq!(intent.conversation_key, question.key);
    assert_eq!(intent.lifecycle, TaskIntentLifecycle::Complete);
}

#[test]
fn feature_completion_requires_lifecycle_and_publication_evidence() {
    let feature = root(WorkKind::Feature, WorkStatus::Done);
    let feature_uid = uuid::Uuid::new_v4().to_string();
    let sha = "a".repeat(64);
    let current = crate::core::lifecycle::CurrentImplementationEvidenceV1 {
        schema_version: 1,
        uid: uuid::Uuid::new_v4().to_string(),
        feature_uid: feature_uid.clone(),
        task_uid: uuid::Uuid::new_v4().to_string(),
        work_item_uid: uuid::Uuid::new_v4().to_string(),
        repository_id: "root".into(),
        base_commit_sha: "b".repeat(40),
        pull_request_head_sha: "c".repeat(40),
        merge_commit_sha: "d".repeat(40),
        checked_out_head_sha: "d".repeat(40),
        specification_revision: 1,
        specification_hash: sha.clone(),
        revision: 1,
        recorded_at_ms: 1,
    };
    let deployment = crate::core::lifecycle::DeploymentEvidenceV1 {
        schema_version: 1,
        feature_uid,
        current_implementation_revision: current.revision,
        specification_revision: current.specification_revision,
        specification_hash: current.specification_hash.clone(),
        merge_commit_sha: current.merge_commit_sha.clone(),
        repository_id: current.repository_id.clone(),
        deployment_id: Some(1),
        status_id: Some(1),
        environment: Some("production".into()),
        production_environment: true,
        state: Some("success".into()),
        observed_at_ms: 1,
        revision: 1,
    };
    assert_eq!(
        TaskIntent::project(&feature, &[], None, &[], None)
            .unwrap()
            .lifecycle,
        TaskIntentLifecycle::Unknown,
        "a completed planning card does not prove delivered implementation"
    );
    assert_eq!(
        TaskIntent::project(
            &feature,
            &[],
            Some(FeaturePhase::Complete),
            &[TaskPhase::PrPending],
            None,
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::PreparingPullRequest
    );
    assert_eq!(
        TaskIntent::project(
            &feature,
            &[],
            Some(FeaturePhase::Complete),
            &[TaskPhase::PrOpened],
            None,
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::PullRequestOpen
    );
    assert_eq!(
        TaskIntent::project(
            &feature,
            &[],
            Some(FeaturePhase::Complete),
            &[TaskPhase::PrMerged],
            None,
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::Merged
    );
    assert_eq!(
        TaskIntent::project(
            &feature,
            &[],
            Some(FeaturePhase::Complete),
            &[TaskPhase::PrMerged],
            Some((&deployment, &current)),
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::Deployed
    );
}

#[test]
fn blocked_children_and_missing_feature_evidence_do_not_project_success() {
    let feature = root(WorkKind::Feature, WorkStatus::Done);
    assert_eq!(
        TaskIntent::project(
            &feature,
            &[WorkStatus::NeedsAttention],
            Some(FeaturePhase::Complete),
            &[TaskPhase::PrMerged],
            None,
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::NeedsAttention
    );
    assert_eq!(
        TaskIntent::project(&feature, &[], Some(FeaturePhase::Complete), &[], None,)
            .unwrap()
            .lifecycle,
        TaskIntentLifecycle::Unknown
    );
}

#[test]
fn non_feature_intents_cannot_complete_with_unfinished_or_failed_executable_children() {
    let question = root(WorkKind::Question, WorkStatus::Done);
    for (phase, expected) in [
        (TaskPhase::Running, TaskIntentLifecycle::InProgress),
        (TaskPhase::Failed, TaskIntentLifecycle::Failed),
    ] {
        assert_eq!(
            TaskIntent::project(&question, &[], None, &[phase], None)
                .unwrap()
                .lifecycle,
            expected
        );
    }
    assert_eq!(
        TaskIntent::project_with_cancelled(&question, &[], None, &[], None, true)
            .unwrap()
            .lifecycle,
        TaskIntentLifecycle::Cancelled
    );
}

#[test]
fn an_advanced_child_does_not_hide_unfinished_parallel_work() {
    let feature = root(WorkKind::Feature, WorkStatus::Done);
    for phase in [
        TaskPhase::Ready,
        TaskPhase::Running,
        TaskPhase::AwaitingRuntimeQa,
    ] {
        assert_eq!(
            TaskIntent::project(
                &feature,
                &[],
                Some(FeaturePhase::Complete),
                &[TaskPhase::PrMerged, phase],
                None,
            )
            .unwrap()
            .lifecycle,
            if phase == TaskPhase::AwaitingRuntimeQa {
                TaskIntentLifecycle::AwaitingReview
            } else {
                TaskIntentLifecycle::InProgress
            },
            "a merged child cannot hide a sibling at {phase:?}"
        );
    }
    assert_eq!(
        TaskIntent::project(
            &feature,
            &[],
            Some(FeaturePhase::Complete),
            &[TaskPhase::PrOpened, TaskPhase::PrMerged],
            None,
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::PullRequestOpen
    );
}

#[test]
fn cancelled_and_failed_children_do_not_hide_parallel_outcomes() {
    let question = root(WorkKind::Question, WorkStatus::Done);
    for (phases, expected) in [
        (
            vec![TaskPhase::Cancelled, TaskPhase::Running],
            TaskIntentLifecycle::InProgress,
        ),
        (
            vec![TaskPhase::Cancelled, TaskPhase::Done],
            TaskIntentLifecycle::PartiallyComplete,
        ),
        (
            vec![TaskPhase::Failed, TaskPhase::Running],
            TaskIntentLifecycle::InProgress,
        ),
        (
            vec![TaskPhase::Failed, TaskPhase::Done],
            TaskIntentLifecycle::PartiallyComplete,
        ),
        (vec![TaskPhase::Cancelled], TaskIntentLifecycle::Cancelled),
        (vec![TaskPhase::Failed], TaskIntentLifecycle::Failed),
    ] {
        assert_eq!(
            TaskIntent::project(&question, &[], None, &phases, None)
                .unwrap()
                .lifecycle,
            expected,
            "unexpected aggregate for {phases:?}"
        );
    }
}

#[test]
fn unresolved_linked_decisions_block_completion() {
    let question = root(WorkKind::Question, WorkStatus::Done);
    assert_eq!(
        TaskIntent::project_with_decisions(
            &question,
            &[],
            &[crate::domain::item::ItemStatus::Open],
            None,
            &[],
            None,
            false,
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::NeedsAttention
    );
    assert_eq!(
        TaskIntent::project_with_decisions(
            &question,
            &[],
            &[crate::domain::item::ItemStatus::Resolved],
            None,
            &[],
            None,
            false,
        )
        .unwrap()
        .lifecycle,
        TaskIntentLifecycle::Complete
    );
}
