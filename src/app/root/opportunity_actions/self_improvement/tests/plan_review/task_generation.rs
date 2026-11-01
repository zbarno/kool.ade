use super::*;

#[path = "task_generation/graph.rs"]
mod graph;
#[path = "task_generation/harness.rs"]
mod harness;

pub(super) fn generate_and_sync(
    app: &mut crate::app::root::KooladeApp,
    planning: &crate::artifacts::planning_store::PlanningStore,
    feature_id: &str,
    feature_uid: &str,
    run_uid: &str,
) {
    let task_key = match &app.screen {
        Screen::Connected(project) => project
            .planning_work
            .iter()
            .find(|work| {
                work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                    && work.feature_uid.as_deref() == Some(feature_uid)
            })
            .map(|work| work.key.clone())
            .expect("reviewed Pursue creates a task-generation card"),
        Screen::Welcome => panic!("project remains connected"),
    };

    let Screen::Connected(project) = &mut app.screen else {
        panic!("project remains connected")
    };
    project.queue.auto_build = false;
    project.queue.running = false;
    project
        .state
        .resync()
        .expect("reviewed plan and readiness reload from the planning store");
    assert!(matches!(
        &app.screen,
        Screen::Connected(project)
            if project.state.workflow.ready(project.state.planning_contract())
    ));
    assert!(app.feature_approved(feature_id));
    let graph = graph::decomposer_graph(app);
    app.task_harness = Some(harness::response_harness(graph));
    app.start_task_generation(&task_key, feature_id);
    assert!(app.pending_feature_generation.is_none());
    assert!(
        matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_some()),
        "the current approved Evolve plan should start task generation; messages: {:?}",
        app.chat_messages()
    );
    assert!(
        harness::finish_main_turn(app),
        "task generation should apply; messages: {:?}",
        app.chat_messages()
    );
    assert!(matches!(
        &app.screen,
        Screen::Connected(project)
            if crate::app::root::has_current_task_batch(project)
    ));

    crate::app::root::self_improvement_test_support::synchronize_run(app, planning, run_uid)
        .unwrap();
    let run = SelfImprovementStore::new(planning).load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::Ready);
    assert_eq!(run.feature_uid, feature_uid);
    assert!(!run.task_uids.is_empty());
    assert!(run.owner_task_uid.is_some());
    assert!(run.plan_approval_hash.is_some());
    assert!(run.independent_plan_review_uid.is_some());
    assert!(
        run.clone_uid.is_none(),
        "planning must not allocate a clone"
    );

    let runs = SelfImprovementStore::new(planning);
    let owner_task_uid = run
        .owner_task_uid
        .as_deref()
        .expect("the approved Evolve plan has an owner task");
    let lifecycle = LifecycleStore::new(planning);
    let owner_task = lifecycle.load_task(owner_task_uid).unwrap();
    assert_eq!(owner_task.feature_uid, run.feature_uid);
    let owner_planning_item_uid = owner_task
        .planning_item_uid
        .as_deref()
        .expect("the generated task maps to a planning item");
    let owner_document = match &app.screen {
        Screen::Connected(project) => project
            .task_documents
            .iter()
            .find(|document| {
                document
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.uid == owner_planning_item_uid)
            })
            .expect("the generated owner task is available in the app task board"),
        Screen::Welcome => panic!("the app remains connected after task generation"),
    };
    let code_repository_id =
        crate::core::implementation::task_repository_id_with_store(planning, &owner_document.path)
            .unwrap();
    assert_eq!(
        owner_document.metadata.as_ref().unwrap().repository_id,
        code_repository_id
    );
    let manifest = crate::core::project_repos::ProjectManifest::load(planning).unwrap();
    let repository = manifest
        .repositories
        .iter()
        .find(|repository| repository.id == code_repository_id)
        .expect("the generated task repository is in the app manifest");
    let workspace_uid = planning.project_id.hyphenated().to_string();
    let repository_uid =
        crate::core::project_repos::stable_uid(&workspace_uid, "repository", &code_repository_id);
    assert_eq!(repository_uid, run.target_repository_uid);
    assert_eq!(repository.remote, run.canonical_remote);
    assert!(
        crate::app::root::implementation_controller::authorize_self_improvement_task_start_for_test(
            planning,
            match &app.screen {
                Screen::Connected(project) => &project.task_documents,
                Screen::Welcome => panic!("the app remains connected after task generation"),
            },
            &owner_document.path,
            &code_repository_id,
        )
        .expect("the app-generated task is dispatchable for its manifest target"),
        "the controller must identify the generated task as a self-improvement run"
    );
    assert!(
        runs.authorize_task_start(
            &run.feature_uid,
            Some("unplanned-task"),
            &repository_uid,
            &repository.remote,
        )
        .is_err()
    );
    let other_repository = format!("{}-other", repository_uid);
    assert!(
        runs.authorize_task_start(
            &run.feature_uid,
            Some(owner_task_uid),
            &other_repository,
            &repository.remote,
        )
        .is_err()
    );
    assert!(
        runs.authorize_task_start(
            &run.feature_uid,
            Some(owner_task_uid),
            &repository_uid,
            "https://github.com/unapproved/other",
        )
        .is_err()
    );

    let owner_task_path = owner_document.path.clone();
    let mut drifted_manifest = manifest.clone();
    drifted_manifest
        .repositories
        .iter_mut()
        .find(|candidate| candidate.id == code_repository_id)
        .expect("the approved repository remains in the manifest")
        .remote = "https://github.com/attacker/kool.ade".into();
    planning
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.to_owned(),
                serde_json::to_vec(&drifted_manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
    set_app_repo_origin(app, "https://github.com/attacker/kool.ade");
    app.start_implementation_with_capabilities(
        owner_task_path,
        true,
        crate::harness::runtime_capabilities::RuntimeCapabilities {
            planning_access:
                crate::harness::runtime_capabilities::PlanningAccess::SandboxedRepositoryReads,
            implementation: true,
        },
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("the app remains connected after a denied dispatch")
    };
    assert!(
        project.active_implementations.is_empty(),
        "drifted manifest started workers {:?}; queue error: {}",
        project.active_implementations.keys().collect::<Vec<_>>(),
        project.queue.last_error
    );
    assert!(project.queue.in_flight.is_empty());
    assert!(
        project
            .queue
            .last_error
            .contains("Self-improvement target check blocked implementation"),
        "unexpected controller denial: {}",
        project.queue.last_error
    );
    let still_ready = runs.load(run_uid).unwrap();
    assert_eq!(still_ready.phase, Phase::Ready);
    assert!(still_ready.clone_uid.is_none());

    planning
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.to_owned(),
                serde_json::to_vec(&manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
    set_app_repo_origin(app, "https://github.com/zbarno/kool.ade");

    let feature = lifecycle.load_feature(feature_uid).unwrap();
    assert_eq!(feature.phase, FeaturePhase::Executing);
    assert_eq!(run.task_uids, feature.task_uids);
}

fn set_app_repo_origin(app: &crate::app::root::KooladeApp, remote: &str) {
    let repo_root = match &app.screen {
        Screen::Connected(project) => &project.state.repo_root,
        Screen::Welcome => panic!("fixture should remain connected"),
    };
    let output = std::process::Command::new("git")
        .args(["remote", "set-url", "origin", remote])
        .current_dir(repo_root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cannot update the local fixture origin: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
