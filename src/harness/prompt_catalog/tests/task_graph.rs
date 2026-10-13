use super::*;
use std::collections::HashMap;

fn uid(number: u128) -> String {
    uuid::Uuid::from_u128(number).hyphenated().to_string()
}

fn subtask(
    number: u128,
    parent: &str,
    deps: &[String],
    parallelizable: bool,
    paths: &[&str],
) -> SubtaskDraftV1 {
    SubtaskDraftV1 {
        uid: uid(number),
        parent_uid: parent.to_owned(),
        title: format!("Subtask {number}"),
        goal: "Complete one part of the parent deliverable.".into(),
        acceptance_criteria: vec!["The subtask result is observable.".into()],
        tests: vec!["Run its focused test.".into()],
        depends_on: deps.to_vec(),
        parallelizable,
        affected_paths: paths.iter().map(|path| (*path).to_owned()).collect(),
    }
}

fn task(
    number: u128,
    requirement: &str,
    dependencies: &[String],
    children: Vec<SubtaskDraftV1>,
    paths: &[&str],
) -> TaskDraftV1 {
    TaskDraftV1 {
        uid: uid(number),
        title: format!("Task {number}"),
        goal: "Deliver one standalone approved outcome.".into(),
        intent: "Implement the approved user-visible capability.".into(),
        non_goals: vec!["Do not broaden the approved feature.".into()],
        requirements: vec![requirement.into()],
        acceptance_criteria: vec!["The approved behavior is observable.".into()],
        tests: vec!["Run the focused integration test.".into()],
        relevant_files: paths.iter().map(|path| (*path).to_owned()).collect(),
        dependencies: dependencies.to_vec(),
        repository_id: uid(90),
        source_branch: "feature/source".into(),
        source_sha: Some("a".repeat(40)),
        destination_branch: "main".into(),
        estimated_concurrency_risks: vec!["Changes are isolated to the listed paths.".into()],
        subtasks: children,
    }
}

fn graph(tasks: Vec<TaskDraftV1>) -> TaskGraphV1 {
    TaskGraphV1 {
        feature_uid: uid(80),
        approved_spec_ref: uid(79),
        decision_refs: vec![uid(78)],
        approved_plan_hash: "a".repeat(64),
        approved_requirement_ids: vec!["REQ-1".into(), "REQ-2".into()],
        tasks,
    }
}

fn valid_graph() -> TaskGraphV1 {
    let a = uid(1);
    let a1 = uid(11);
    let b = uid(2);
    graph(vec![
        task(
            1,
            "REQ-1",
            &[],
            vec![
                subtask(11, &a, &[], false, &["src/a"]),
                subtask(12, &a, &[a1], false, &["src/b"]),
            ],
            &["src/a", "src/b"],
        ),
        task(
            2,
            "REQ-2",
            &[],
            vec![subtask(21, &b, &[], true, &["src/c"])],
            &["src/c"],
        ),
    ])
}

#[test]
fn decomposer_output_accepts_two_standalone_tasks_and_nested_dag() {
    valid_graph().validate().unwrap();
}

#[test]
fn decomposer_output_rejects_missing_refs_cycles_and_wrong_ownership() {
    let mut value = valid_graph();
    value.tasks[1].dependencies.push(uid(99));
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[0].dependencies.push(uid(2));
    value.tasks[1].dependencies.push(uid(1));
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[0].subtasks[0].depends_on.push(uid(12));
    value.tasks[0].subtasks[1].depends_on.push(uid(11));
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[0].subtasks[0].parent_uid = uid(2);
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[0].subtasks[0].depends_on.push(uid(21));
    assert!(value.validate().is_err());
}

#[test]
fn decomposer_output_rejects_unowned_requirements_parallel_path_conflicts_and_bad_source_identity()
{
    let mut value = valid_graph();
    value.approved_requirement_ids.push("REQ-3".into());
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[1].requirements.push("REQ-1".into());
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[0]
        .subtasks
        .push(subtask(13, &uid(1), &[], true, &["src/a/child.rs"]));
    value.tasks[0].subtasks[0].parallelizable = true;
    value.tasks[0].subtasks[1].parallelizable = true;
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[1].source_sha = Some("b".repeat(40));
    assert!(value.validate().is_err());

    let mut value = valid_graph();
    value.tasks[0].relevant_files.push("../outside.rs".into());
    assert!(value.validate().is_err());
}

#[test]
fn role_output_validation_binds_feature_requirements_and_repository_context() {
    let value = valid_graph();
    let encoded = serde_json::to_string(&value).unwrap();
    let expected_feature_uid = uid(80);
    let context = OutputValidationContext {
        expected_feature_uid: Some(&expected_feature_uid),
        expected_approved_spec_ref: Some(&uid(79)),
        expected_approved_plan_hash: Some(&"a".repeat(64)),
        approved_decision_refs: vec![uid(78)],
        required_requirements: vec!["REQ-1".into(), "REQ-2".into()],
        allowed_repositories: HashMap::from([(uid(90), "https://example.invalid/repo".into())]),
        approved_branch_targets: HashMap::from([(
            uid(90),
            ("feature/source".into(), "main".into()),
        )]),
        repository_source_shas: HashMap::from([(uid(90), "a".repeat(40))]),
        ..Default::default()
    };
    assert!(matches!(
        validate_output_with_context(PromptRole::Decomposer, &encoded, &context),
        Ok(RoleOutputV1::TaskGraph(_))
    ));

    let wrong_feature_uid = uid(81);
    let wrong_feature = OutputValidationContext {
        expected_feature_uid: Some(&wrong_feature_uid),
        ..context.clone()
    };
    assert!(
        validate_output_with_context(PromptRole::Decomposer, &encoded, &wrong_feature).is_err()
    );

    let unknown_repo = OutputValidationContext {
        allowed_repositories: HashMap::from([(uid(91), "https://example.invalid/other".into())]),
        ..context.clone()
    };
    assert!(validate_output_with_context(PromptRole::Decomposer, &encoded, &unknown_repo).is_err());

    let wrong_branch = OutputValidationContext {
        approved_branch_targets: HashMap::from([(
            uid(90),
            ("feature/other".into(), "main".into()),
        )]),
        ..context.clone()
    };
    assert!(validate_output_with_context(PromptRole::Decomposer, &encoded, &wrong_branch).is_err());

    let wrong_source = OutputValidationContext {
        repository_source_shas: HashMap::from([(uid(90), "b".repeat(40))]),
        ..context.clone()
    };
    assert!(validate_output_with_context(PromptRole::Decomposer, &encoded, &wrong_source).is_err());

    let mut missing_source_sha = value.clone();
    missing_source_sha.tasks[0].source_sha = None;
    assert!(
        validate_output_with_context(
            PromptRole::Decomposer,
            &serde_json::to_string(&missing_source_sha).unwrap(),
            &context,
        )
        .is_err()
    );

    let wrong_plan = OutputValidationContext {
        expected_approved_plan_hash: Some(&"b".repeat(64)),
        ..context.clone()
    };
    assert!(validate_output_with_context(PromptRole::Decomposer, &encoded, &wrong_plan).is_err());

    let wrong_decisions = OutputValidationContext {
        approved_decision_refs: vec![uid(77)],
        ..context.clone()
    };
    assert!(
        validate_output_with_context(PromptRole::Decomposer, &encoded, &wrong_decisions).is_err()
    );
}
