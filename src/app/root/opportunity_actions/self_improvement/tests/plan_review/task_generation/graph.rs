use super::super::*;

pub(super) fn decomposer_graph(app: &crate::app::root::KooladeApp) -> String {
    use crate::harness::prompt_catalog::{TaskDraftV1, TaskGraphV1};

    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    let state = &project.state;
    let (feature_id, markdown) = state.active_feature.as_ref().expect("active feature");
    let identity = ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .expect("pursued feature has stable identity");
    let contract = crate::core::workflow::feature_contract(markdown);
    let snapshot = plan_snapshot::from_feature_with_author(
        &state.planning_store,
        feature_id,
        &identity,
        markdown,
        &contract,
    )
    .unwrap();
    let feature = LifecycleStore::new(&state.planning_store)
        .load_feature(&identity.uid)
        .unwrap();
    let branches = state
        .workflow
        .feature_branch_targets
        .get(feature_id)
        .expect("task generation persists the selected branch targets");
    let repository = state
        .repositories
        .repositories
        .iter()
        .find(|repository| repository.id == "root")
        .expect("fixture has one root repository");
    assert_eq!(repository.id, "root");
    let run = crate::core::self_improvement::SelfImprovementStore::new(&state.planning_store)
        .run_for_feature(&identity.uid)
        .unwrap()
        .expect("pursued feature retains its approved self-improvement run");
    assert!(
        run.source_ref
            .as_deref()
            .is_none_or(|source_ref| source_ref == branches.source),
        "selected source branch remains consistent with any persisted Evolve target"
    );
    let workspace_uid = state.planning_store.project_id.hyphenated().to_string();
    assert_eq!(
        crate::core::project_repos::stable_uid(&workspace_uid, "repository", &repository.id),
        run.target_repository_uid
    );
    let source_sha = run.source_sha;

    let tasks = snapshot
        .requirements
        .iter()
        .enumerate()
        .map(|(index, requirement)| TaskDraftV1 {
            uid: uuid::Uuid::new_v5(
                &uuid::Uuid::parse_str(&identity.uid).unwrap(),
                requirement.stable_id.as_bytes(),
            )
            .hyphenated()
            .to_string(),
            title: format!("Implement requirement {}", index + 1),
            goal: requirement.text.clone(),
            intent: "Implement the independently reviewed requirement.".into(),
            non_goals: vec!["Do not expand the approved plan.".into()],
            requirements: vec![requirement.stable_id.clone()],
            acceptance_criteria: vec!["The approved requirement is observable.".into()],
            tests: vec!["Run the focused requirement test.".into()],
            relevant_files: vec![format!("src/approved_requirement_{index}.rs")],
            dependencies: Vec::new(),
            repository_id: repository.id.clone(),
            source_branch: branches.source.clone(),
            source_sha: Some(source_sha.clone()),
            destination_branch: branches.destination.clone(),
            estimated_concurrency_risks: vec!["The task owns its listed path.".into()],
            subtasks: Vec::new(),
        })
        .collect();
    serde_json::to_string(&TaskGraphV1 {
        feature_uid: identity.uid,
        approved_spec_ref: snapshot.uid,
        decision_refs: feature.decision_refs,
        approved_plan_hash: snapshot.content_hash,
        approved_requirement_ids: snapshot
            .requirements
            .iter()
            .map(|requirement| requirement.stable_id.clone())
            .collect(),
        tasks,
    })
    .unwrap()
}
