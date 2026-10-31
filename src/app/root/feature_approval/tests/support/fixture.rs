use super::super::*;

pub(in crate::app::root::feature_approval::tests) fn decomposer_graph(app: &KooladeApp) -> String {
    use crate::harness::prompt_catalog::{TaskDraftV1, TaskGraphV1};
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    let state = &project.state;
    let (feature_id, markdown) = state.active_feature.as_ref().expect("active feature");
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let contract = crate::core::workflow::feature_contract(markdown);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &state.planning_store,
        feature_id,
        &identity,
        markdown,
        &contract,
    )
    .unwrap();
    let goal = crate::core::lifecycle::LifecycleStore::new(&state.planning_store)
        .load_feature(&identity.uid)
        .unwrap();
    let branches = state
        .workflow
        .feature_branch_targets
        .get(feature_id)
        .unwrap();
    let repository = state
        .repositories
        .repositories
        .iter()
        .find(|repository| repository.id == "root")
        .unwrap();
    let checkout = state
        .repositories
        .target(
            state.planning_store.project_id,
            &state.repo_root,
            &repository.id,
        )
        .unwrap();
    let sha = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            &format!("refs/heads/{}^{{commit}}", branches.source),
        ])
        .current_dir(checkout)
        .output()
        .unwrap();
    assert!(sha.status.success());
    let sha = String::from_utf8(sha.stdout).unwrap().trim().to_owned();
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
            intent: "Implement the approved requirement.".into(),
            non_goals: vec!["Do not expand the approved plan.".into()],
            requirements: vec![requirement.stable_id.clone()],
            acceptance_criteria: vec!["The approved requirement is observable.".into()],
            tests: vec!["Run the focused requirement test.".into()],
            relevant_files: vec![format!("src/approved_requirement_{index}.rs")],
            dependencies: Vec::new(),
            repository_id: repository.id.clone(),
            source_branch: branches.source.clone(),
            source_sha: Some(sha.clone()),
            destination_branch: branches.destination.clone(),
            estimated_concurrency_risks: vec!["The task owns its listed path.".into()],
            subtasks: Vec::new(),
        })
        .collect();
    serde_json::to_string(&TaskGraphV1 {
        feature_uid: identity.uid,
        approved_spec_ref: snapshot.uid,
        decision_refs: goal.decision_refs,
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

pub(crate) fn setup() -> (KooladeApp, std::path::PathBuf, serde_json::Value) {
    let mut app = fixture();
    // Timestamp-only names can collide when nextest starts these fixtures in
    // parallel. Each approval flow owns a fully isolated project and record
    // store, so use a random suffix instead of relying on clock resolution.
    let root = std::env::temp_dir().join(format!(
        "koolade-approval-{}",
        uuid::Uuid::new_v4().hyphenated()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut state = crate::core::state::PlannerState::load(&root).unwrap();
    state.bootstrap_missing().unwrap();
    let dir = root.join("planning/changes/CHG-004-saved-searches");
    std::fs::create_dir_all(&dir).unwrap();
    let mut spec = "# CHG-004: Saved searches\n\n**Status:** Ready\n".to_string();
    for heading in [
        "Intent",
        "Current Behavior",
        "Desired Behavior",
        "Scope",
        "Affected Product Areas",
        "Requirements",
        "Decisions and Assumptions",
        "Acceptance Criteria",
    ] {
        spec.push_str(&format!(
            "\n## {heading}\n\nPersist saved searches and restore them after restarting.\n"
        ));
    }
    spec = spec.replace(
        "## Affected Product Areas\n",
        "## Affected Product Areas\n\n`product:current-capabilities`\n",
    );
    let spec_path = dir.join("specification.md");
    spec = crate::artifacts::product_docs::identity::preserve_feature_identity(
        &spec_path, "CHG-004", &spec,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&spec)
        .unwrap()
        .unwrap();
    spec = crate::domain::ChangeMetadata::write_markdown(
        &spec,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::write(spec_path, &spec).unwrap();
    let mut review: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/interview-ready.json"
    ))
    .unwrap();
    review["updated_specification"] = serde_json::Value::Null;
    review["document_updates"] = serde_json::Value::Null;
    review["interview"]["feature_name"] = "Saved searches (CHG-004)".into();
    state.workflow.brief = Some(serde_json::from_value(review["interview"].clone()).unwrap());
    state.workflow.reviewed_specification = Some("Previous CHG-003 specification".into());
    crate::artifacts::task_docs::save_workflow(&root, &state.workflow).unwrap();
    let mut item = OpenItem::new(
        "CLR-026".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Branch retention choice".into(),
        "Settled".into(),
    );
    item.feature_id = Some("CHG-004".into());
    item.status = crate::domain::ItemStatus::Resolved;
    std::fs::write(
        root.join("planning/resolved-items.json"),
        serde_json::to_vec(&vec![item]).unwrap(),
    )
    .unwrap();
    crate::core::gitops::commit(&root, "fixture", &["planning".into(), "config".into()]).unwrap();
    let feature_identity = crate::domain::ArtifactIdentity::from_markdown(&spec)
        .unwrap()
        .unwrap();
    let contract = crate::core::workflow::feature_contract(&spec);
    let author = crate::core::lifecycle::plan_snapshot::PlanAuthorProvenanceV1 {
        feature_uid: feature_identity.uid,
        approval_contract_sha256: format!("{:x}", sha2::Sha256::digest(contract.as_bytes())),
        author_run_id: "00000000-0000-4000-8000-000000000001".into(),
        author_session_id: "00000000-0000-4000-8000-000000000002".into(),
    };
    crate::core::lifecycle::plan_snapshot::record_author(&state.planning_store, &author).unwrap();
    crate::core::gitops::commit(
        &root,
        "fixture: record authored plan provenance",
        &[state.planning_store.git_path(&format!(
            "state/lifecycle_plan_authors/{}.json",
            author.feature_uid
        ))],
    )
    .unwrap();
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    p.state = crate::core::state::PlannerState::load(&root).unwrap();
    readiness::seed_test_readiness(&p.state.planning_store, "CHG-004", &spec);
    p.state = crate::core::state::PlannerState::load(&root).unwrap();
    p.task_documents.clear();
    p.implementation_states.clear();
    p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
    p.task_chats.remember_response(
        &p.chat_slug,
        "CLR-026",
        vec![ChatMessage::new(
            ChatRole::Agent,
            "Click Approve feature for implementation for CHG-004.",
            None,
        )],
    );
    (app, root, review)
}
