use super::{KooladeApp, Screen};
use crate::{
    artifacts::planning_store::PlanningStore,
    core::{
        implementation::Implementation, lifecycle::LifecycleStore, project_repos::ProjectManifest,
        self_improvement::SelfImprovementStore,
    },
};

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
