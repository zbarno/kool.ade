use super::{Project, RepositoryNameRow, repository_names};
use crate::{AppError, core::gitops};
use std::collections::BTreeMap;

mod project_routes;
use project_routes::paint_routes;
mod agent_guidance;

#[derive(Clone, Default)]
pub struct DlgProjectSettings {
    pub repositories: Vec<RepositoryNameRow>,
    pub harness_routes: BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
    pub feedback: Option<(bool, String)>,
    pub agent_guidance: String,
    pub agent_guidance_enabled: bool,
    pub(super) saved_agent_guidance:
        Option<crate::persistence::project_guidance::ApprovedProjectGuidanceV1>,
    pub(super) approve_agent_guidance: bool,
    pub(super) agent_guidance_error: Option<String>,
    pub(crate) agent_guidance_saved: bool,
}

impl DlgProjectSettings {
    pub fn from_project(project: &Project) -> Self {
        let mut draft = Self {
            repositories: repository_names::rows(&project.state.repositories),
            harness_routes: project.state.harness_routes.routes.clone(),
            feedback: None,
            agent_guidance: String::new(),
            agent_guidance_enabled: false,
            saved_agent_guidance: None,
            approve_agent_guidance: false,
            agent_guidance_error: None,
            agent_guidance_saved: false,
        };
        agent_guidance::load(project, &mut draft);
        draft
    }

    pub fn apply(&mut self, project: &mut Project) -> Result<Option<String>, AppError> {
        let _guard = crate::core::writer_gate::acquire();
        let repositories_changed = repository_names::persist(project, &mut self.repositories)?;
        let routes_changed = self.harness_routes != project.state.harness_routes.routes;
        if !repositories_changed && !routes_changed {
            agent_guidance::apply(project, self)?;
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
        let short_sha = sha.chars().take(7).collect::<String>();
        if let Err(error) = agent_guidance::apply(project, self) {
            return Err(AppError::Other(format!(
                "Shared project settings were saved at checkpoint {short_sha}, but approved guidance was not saved: {}",
                error.detail()
            )));
        }
        Ok(Some(short_sha))
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
    agent_guidance::paint(ui, dialog);
    ui.add_space(12.0);
    ui.label(crate::ui::theme::section_heading("Harness routes"));
    ui.label("Choose project-wide provider routes by role. Task overrides take precedence, followed by these project routes and then Coding tools defaults.");
    let settings = crate::persistence::harness_settings::load().0;
    paint_routes(ui, &mut dialog.harness_routes, &settings);
    ui.add_space(6.0);
    crate::app::dialogs::footers(ui, &dialog.feedback)
}
