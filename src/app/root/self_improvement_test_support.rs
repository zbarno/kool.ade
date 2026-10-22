use super::{KooladeApp, Screen};
use crate::{
    artifacts::planning_store::PlanningStore,
    core::{
        implementation::Implementation, lifecycle::LifecycleStore, project_repos::ProjectManifest,
        self_improvement::SelfImprovementStore,
    },
};

pub(crate) fn app_fixture() -> (KooladeApp, PlanningStore, std::path::PathBuf) {
    let app = super::board_tests::fixture();
    let (planning, repo_root) = match &app.screen {
        Screen::Connected(project) => (
            project.state.planning_store.clone(),
            project.state.repo_root.clone(),
        ),
        _ => panic!("board fixture should be connected"),
    };
    (app, planning, repo_root)
}

pub(crate) fn refresh_planning_state(app: &mut KooladeApp, planning: &PlanningStore) {
    let Screen::Connected(project) = &mut app.screen else {
        panic!("board fixture should be connected")
    };
    project.state.planning_store = planning.clone();
    project.state.repositories = ProjectManifest::load(planning).unwrap();
    project.state.baseline_planning_revision = planning.revision().unwrap();
}

pub(crate) fn decide_product_pursuit(
    app: &mut KooladeApp,
    opportunity_uid: &str,
    expected_revision: u64,
    gh: &std::path::Path,
) -> Option<String> {
    app.decide_opportunity_with_gh(
        opportunity_uid,
        expected_revision,
        crate::harness::prompt_catalog::OpportunityChoice::Pursue,
        None,
        gh,
    );
    app.toasts.last_message().map(|message| message.to_owned())
}

pub(crate) fn app_with_publication(
    planning: &PlanningStore,
    implementation: Implementation,
) -> KooladeApp {
    let mut app = super::board_tests::fixture();
    let manifest = ProjectManifest::load(planning).unwrap();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    project.state.planning_store = planning.clone();
    project.state.repositories = manifest;
    project
        .implementation_states
        .insert(implementation.ticket.clone(), implementation);
    app
}

pub(crate) fn synchronize_run(
    app: &mut KooladeApp,
    planning: &PlanningStore,
    run_uid: &str,
) -> Result<(), String> {
    let Screen::Connected(project) = &mut app.screen else {
        return Err("fixture disconnected during self-improvement sync".into());
    };
    let lifecycle = LifecycleStore::new(planning);
    let runs = SelfImprovementStore::new(planning);
    super::tick::activity::synchronize_self_improvement_run_for_test(
        project, &lifecycle, &runs, run_uid,
    )
}
