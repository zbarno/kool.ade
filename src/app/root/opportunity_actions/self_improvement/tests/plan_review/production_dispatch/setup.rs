use super::super::{fixture, reviewer, task_generation};
use crate::{
    app::root::{KooladeApp, Screen},
    artifacts::{planning_store::PlanningStore, task_docs::TaskMetadata},
    core::{
        lifecycle::{FeaturePhase, LifecycleStore, plan_snapshot},
        opportunity::OpportunityStore,
        self_improvement::{
            SelfImprovementPhaseV1 as Phase, SelfImprovementRunV1, SelfImprovementStore,
        },
    },
    ui::Surface,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) struct ReadyTask {
    pub(super) _app: KooladeApp,
    pub(super) planning: PlanningStore,
    pub(super) repo_root: PathBuf,
    pub(super) run: SelfImprovementRunV1,
    pub(super) ticket: String,
    pub(super) metadata: TaskMetadata,
}

pub(super) fn ready_task(gh: &std::path::Path) -> ReadyTask {
    let mut app = crate::app::root::board_tests::fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.queue.auto_build = false;
        project.queue.running = false;
    }
    let opportunity_uid = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        let remote = std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/zbarno/kool.ade",
            ])
            .current_dir(&project.state.repo_root)
            .output()
            .unwrap();
        assert!(remote.status.success());
        super::super::super::app_product_proposal(project)
    };
    let (planning, opportunity_revision, repo_root) = match &mut app.screen {
        Screen::Connected(project) => {
            project.state.resync().unwrap();
            project.state.bootstrap_missing().unwrap();
            let planning = project.state.planning_store.clone();
            let (_, revision) = OpportunityStore::new(&planning)
                .load(&opportunity_uid)
                .unwrap();
            (planning, revision, project.state.repo_root.clone())
        }
        Screen::Welcome => panic!("fixture should remain connected"),
    };
    let message = crate::app::root::self_improvement_test_support::decide_product_pursuit(
        &mut app,
        &opportunity_uid,
        opportunity_revision,
        gh,
    );
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("self-improvement run"))
    );

    let (feature_id, feature_uid, run_uid) = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should remain connected")
        };
        let feature_id = project.state.active_feature.as_ref().unwrap().0.clone();
        let markdown = &project.state.active_feature.as_ref().unwrap().1;
        let feature_uid = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .unwrap()
            .uid;
        let decision = OpportunityStore::new(&planning)
            .decision_for_opportunity(&opportunity_uid)
            .unwrap()
            .unwrap();
        let run_uid = SelfImprovementStore::run_uid_for_decision(&decision.uid).unwrap();
        let run = SelfImprovementStore::new(&planning).load(&run_uid).unwrap();
        assert_eq!(run.phase, Phase::Planning);
        assert!(run.task_uids.is_empty());
        fixture::prepare_reviewable_plan(project, &feature_id);
        (feature_id, feature_uid, run_uid)
    };

    app.dispatch(crate::ui::ApplicationCommand::ChooseFeaturePlan {
        id: feature_id.clone(),
        plan_id: "B".into(),
    });
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    fixture::prepare_plan_review_snapshot(project, &feature_id);
    app.approve_feature_action_with_harness_for_test(
        &feature_id,
        Box::new(reviewer::PassingPlanReviewer),
    );
    let context = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.tick(0.016, &context);
        if !matches!(&app.screen, Screen::Connected(project) if project.activity.plan_review.is_some())
        {
            break;
        }
        assert!(Instant::now() < deadline, "plan review finishes in budget");
        std::thread::sleep(Duration::from_millis(5));
    }
    let snapshot = {
        let Screen::Connected(project) = &app.screen else {
            panic!("fixture should remain connected")
        };
        let markdown = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == &feature_id)
            .unwrap()
            .1
            .clone();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
            .unwrap()
            .unwrap();
        plan_snapshot::from_feature_with_author(
            &planning,
            &feature_id,
            &identity,
            &markdown,
            &crate::core::workflow::feature_contract(&markdown),
        )
        .unwrap()
    };
    assert!(
        plan_snapshot::plan_review_receipt(&planning, &snapshot)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        LifecycleStore::new(&planning)
            .load_feature(&feature_uid)
            .unwrap()
            .phase,
        FeaturePhase::ReadyToDecompose
    );
    app.approve_feature_from_board(&feature_id);
    crate::app::root::self_improvement_test_support::synchronize_run(&mut app, &planning, &run_uid)
        .unwrap();
    task_generation::generate_and_sync(&mut app, &planning, &feature_id, &feature_uid, &run_uid);

    let run = SelfImprovementStore::new(&planning).load(&run_uid).unwrap();
    assert_eq!(run.phase, Phase::Ready);
    let lifecycle = LifecycleStore::new(&planning);
    let owner = lifecycle
        .load_task(run.owner_task_uid.as_deref().unwrap())
        .unwrap();
    let item_uid = owner.planning_item_uid.as_deref().unwrap();
    let (ticket, metadata) = {
        let Screen::Connected(project) = &app.screen else {
            panic!("fixture should remain connected")
        };
        let document = project
            .task_documents
            .iter()
            .find(|document| {
                document
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.uid == item_uid)
            })
            .unwrap();
        (
            document.path.clone(),
            document.metadata.as_ref().unwrap().clone(),
        )
    };
    ReadyTask {
        _app: app,
        planning,
        repo_root,
        run,
        ticket,
        metadata,
    }
}
