use super::{Project, RepositoryNameRow, repository_names};
use crate::{AppError, core::gitops};
use std::collections::BTreeMap;

mod project_routes;
use project_routes::paint_routes;

#[derive(Clone, Default)]
pub struct DlgProjectSettings {
    pub repositories: Vec<RepositoryNameRow>,
    pub harness_routes: BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
    pub feedback: Option<(bool, String)>,
}

impl DlgProjectSettings {
    pub fn from_project(project: &Project) -> Self {
        Self {
            repositories: repository_names::rows(&project.state.repositories),
            harness_routes: project.state.harness_routes.routes.clone(),
            feedback: None,
        }
    }

    pub fn apply(&mut self, project: &mut Project) -> Result<Option<String>, AppError> {
        let _guard = crate::core::writer_gate::acquire();
        let repositories_changed = repository_names::persist(project, &mut self.repositories)?;
        let routes_changed = self.harness_routes != project.state.harness_routes.routes;
        if !repositories_changed && !routes_changed {
            return Ok(None);
        }
        let mut paths = Vec::new();
        if repositories_changed {
            paths.push(
                project
                    .state
                    .planning_store
                    .git_path(crate::artifacts::planning_store::paths::PROJECT_MANIFEST),
            );
        }
        if routes_changed {
            let updated =
                crate::persistence::project_harness_routes::ProjectHarnessRoutes::with_routes(
                    self.harness_routes.clone(),
                );
            let (changed, revision) = updated
                .save(
                    &project.state.planning_store,
                    &project.state.baseline_planning_revision,
                )
                .map_err(|error| crate::AppError::Other(error.to_string()))?;
            project.state.harness_routes = updated;
            project.state.baseline_planning_revision = revision;
            paths.extend(
                changed
                    .iter()
                    .map(|path| project.state.planning_store.git_path(path)),
            );
        }
        let sha = gitops::commit(
            &project.state.planning_store.git_root(),
            "settings: update project configuration",
            &paths,
        )?;
        project.refresh_git();
        Ok(Some(sha.chars().take(7).collect()))
    }
}

pub fn paint_project_settings_card(
    ui: &mut egui::Ui,
    dialog: &mut DlgProjectSettings,
) -> (bool, bool) {
    ui.label(crate::ui::theme::section_heading("Repository names"));
    ui.label("Choose shared display names for this project's registered repositories.");
    ui.add_space(8.0);
    repository_names::paint(ui, &mut dialog.repositories);
    ui.add_space(12.0);
    ui.label(crate::ui::theme::section_heading("Harness routes"));
    ui.label("Choose project-wide provider routes by role. Task overrides take precedence, followed by these project routes and then Coding tools defaults.");
    let settings = crate::persistence::harness_settings::load().0;
    paint_routes(ui, &mut dialog.harness_routes, &settings);
    ui.add_space(6.0);
    crate::app::dialogs::footers(ui, &dialog.feedback)
}
