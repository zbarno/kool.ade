use super::*;
use crate::harness::prompt_catalog::{SubtaskDraftV1, TaskDraftV1, TaskGraphV1};

fn uid(number: u128) -> String {
    uuid::Uuid::from_u128(number).hyphenated().to_string()
}

fn child(id: u128, parent: &str, depends_on: &[String]) -> SubtaskDraftV1 {
    SubtaskDraftV1 {
        uid: uid(id),
        parent_uid: parent.into(),
        title: format!("Child {id}"),
        goal: "Deliver the owned part of the parent task.".into(),
        acceptance_criteria: vec!["The child behavior is observable.".into()],
        tests: vec!["Run the focused child test.".into()],
        depends_on: depends_on.into(),
        parallelizable: false,
        affected_paths: vec![format!("src/child_{id}.rs")],
    }
}

fn root(
    id: u128,
    requirements: &[&str],
    dependencies: &[String],
    children: Vec<SubtaskDraftV1>,
) -> TaskDraftV1 {
    TaskDraftV1 {
        uid: uid(id),
        title: format!("Root {id}"),
        goal: "Deliver one approved standalone outcome.".into(),
        intent: "Implement the approved outcome.".into(),
        non_goals: vec!["Do not expand the plan.".into()],
        requirements: requirements.iter().map(|item| (*item).into()).collect(),
        acceptance_criteria: vec!["The outcome can be verified.".into()],
        tests: vec!["Run the focused test.".into()],
        relevant_files: vec![format!("src/root_{id}.rs")],
        dependencies: dependencies.into(),
        repository_id: "root".into(),
        source_branch: "feature/source".into(),
        source_sha: Some("a".repeat(40)),
        destination_branch: "main".into(),
        estimated_concurrency_risks: vec!["The listed paths define the change boundary.".into()],
        subtasks: children,
    }
}

#[test]
fn flatten_preserves_stable_uids_parent_ownership_and_root_dependency_order() {
    let root_a = uid(1);
    let graph = TaskGraphV1 {
        feature_uid: uid(100),
        approved_spec_ref: uid(101),
        decision_refs: vec![uid(102)],
        approved_plan_hash: "a".repeat(64),
        approved_requirement_ids: vec!["R1".into(), "R2".into()],
        tasks: vec![
            root(
                1,
                &["R1"],
                &[],
                vec![child(11, &root_a, &[]), child(12, &root_a, &[])],
            ),
            root(2, &["R2"], &[root_a], vec![]),
        ],
    };
    graph.validate().unwrap();
    let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        "F-100",
        &crate::domain::ArtifactIdentity { uid: uid(100), display_id: "F-100".into(), title: "Feature".into(), parent_uid: None },
        "# F-100: Feature\n\n## Intent\n\nDeliver the approved outcomes.\n\n## Desired Behavior\n\nThe approved outcomes work.\n\n## Current Behavior\n\nThe outcomes are missing.\n\n## Scope\n\n- Build outcome A.\n- Build outcome B.\n\n## Requirements\n\n- R1 (MUST). Build A.\n- R2 (MUST). Build B.\n\n## Acceptance Criteria\n\n1. Outcome A works.\n2. Outcome B works.\n",
        "approved contract",
    ).unwrap();
    let brief = InterviewBrief {
        in_scope: vec!["Build outcome A.".into(), "Build outcome B.".into()],
        success_criteria: vec!["Outcome A works.".into(), "Outcome B works.".into()],
        ..Default::default()
    };
    let (_, stories) = flatten(graph, &snapshot, &brief).unwrap();
    assert_eq!(stories.len(), 4);
    assert_eq!(stories[0].graph_uid.as_deref(), Some(uid(1).as_str()));
    assert_eq!(stories[0].intent, "Implement the approved outcome.");
    assert!(stories[0].context.contains("Do not expand the plan."));
    assert_eq!(stories[1].graph_uid.as_deref(), Some(uid(11).as_str()));
    assert_eq!(stories[1].parent_task_index, Some(1));
    assert_eq!(stories[2].graph_uid.as_deref(), Some(uid(12).as_str()));
    assert_eq!(stories[2].parent_task_index, Some(1));
    assert_eq!(stories[2].dependencies, vec![2]);
    assert_eq!(stories[3].graph_uid.as_deref(), Some(uid(2).as_str()));
    assert_eq!(stories[3].dependencies, vec![1]);
    assert_eq!(
        stories
            .iter()
            .filter(|story| story.parent_task_index.is_none())
            .count(),
        2
    );
}
