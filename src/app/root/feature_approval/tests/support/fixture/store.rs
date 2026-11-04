use super::super::super::*;

pub(crate) fn private_store_root(app: &KooladeApp) -> std::path::PathBuf {
    match &app.screen {
        Screen::Connected(project) => project.state.planning_store.root.clone(),
        Screen::Welcome => panic!("fixture remains connected"),
    }
}

pub(crate) fn planning_store(app: &KooladeApp) -> crate::artifacts::planning_store::PlanningStore {
    match &app.screen {
        Screen::Connected(project) => project.state.planning_store.clone(),
        Screen::Welcome => panic!("fixture remains connected"),
    }
}
