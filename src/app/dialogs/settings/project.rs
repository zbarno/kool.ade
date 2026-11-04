use super::{Project, RepositoryNameRow, repository_names};
use crate::AppError;
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
    pub new_repository_id: String,
    pub new_repository_role: String,
    pub new_repository_remote: String,
    pub new_repository_checkout: Option<std::path::PathBuf>,
    pub register_repository_requested: bool,
}

impl DlgProjectSettings {
    pub fn from_project(project: &Project) -> Self {
        let checkout_map = crate::core::project_repos::RepositoryCheckoutMapV1::load(
            &project.state.planning_store,
        );
        let mut draft = Self {
            repositories: repository_names::rows(
                &project.state.repositories,
                checkout_map.as_ref().unwrap_or(&Default::default()),
            ),
            harness_routes: project.state.harness_routes.routes.clone(),
            feedback: checkout_map.err().map(|error| {
                (
                    false,
                    format!("Repository checkout settings could not be read: {error}"),
                )
            }),
            agent_guidance: String::new(),
            agent_guidance_enabled: false,
            saved_agent_guidance: None,
            approve_agent_guidance: false,
            agent_guidance_error: None,
            agent_guidance_saved: false,
            new_repository_id: String::new(),
            new_repository_role: String::new(),
            new_repository_remote: String::new(),
            new_repository_checkout: None,
            register_repository_requested: false,
        };
        agent_guidance::load(project, &mut draft);
        draft
    }

    pub fn apply(&mut self, project: &mut Project) -> Result<Option<String>, AppError> {
        let _guard = crate::core::writer_gate::acquire();
        let mut repositories_changed = repository_names::persist(project, &mut self.repositories)?;
        if self.register_repository_requested {
            let path = self.new_repository_checkout.as_deref().ok_or_else(|| {
                AppError::Other("Choose the secondary repository's local checkout first.".into())
            })?;
            let (manifest, revision) = crate::core::project_repos::register_repository_checkout(
                &project.state.planning_store,
                &project.state.repositories,
                &project.state.repo_root,
                crate::core::project_repos::RepositoryCheckoutRegistration {
                    id: &self.new_repository_id,
                    role: &self.new_repository_role,
                    remote: &self.new_repository_remote,
                    path,
                },
                &project.state.baseline_planning_revision,
            )
            .map_err(|error| AppError::Other(error.to_string()))?;
            project.state.repositories = manifest;
            project.state.baseline_planning_revision = revision;
            if let Ok(checkouts) = crate::core::project_repos::RepositoryCheckoutMapV1::load(
                &project.state.planning_store,
            ) {
                self.repositories = repository_names::rows(&project.state.repositories, &checkouts);
            }
            self.new_repository_id.clear();
            self.new_repository_role.clear();
            self.new_repository_remote.clear();
            self.new_repository_checkout = None;
            self.register_repository_requested = false;
            repositories_changed = true;
        }
        let routes_changed = self.harness_routes != project.state.harness_routes.routes;
        if !repositories_changed && !routes_changed {
            agent_guidance::apply(project, self)?;
            return Ok(None);
        }
        if routes_changed {
            let updated =
                crate::persistence::project_harness_routes::ProjectHarnessRoutes::with_routes(
                    self.harness_routes.clone(),
                );
            let (_, revision) = updated
                .save(
                    &project.state.planning_store,
                    &project.state.baseline_planning_revision,
                )
                .map_err(|error| crate::AppError::Other(error.to_string()))?;
            project.state.harness_routes = updated;
            project.state.baseline_planning_revision = revision;
        }
        let revision = project
            .state
            .planning_store
            .revision()
            .map_err(|error| crate::AppError::Other(error.to_string()))?;
        project
            .state
            .baseline_planning_revision
            .clone_from(&revision);
        let short_revision = revision
            .strip_prefix("sha256:")
            .unwrap_or(&revision)
            .chars()
            .take(7)
            .collect::<String>();
        if let Err(error) = agent_guidance::apply(project, self) {
            return Err(AppError::Other(format!(
                "Private project settings were saved at local state revision {short_revision}, but approved guidance was not saved: {}",
                error.detail()
            )));
        }
        Ok(Some(short_revision))
    }
}

pub fn paint_project_settings_card(
    ui: &mut egui::Ui,
    dialog: &mut DlgProjectSettings,
) -> (bool, bool) {
    ui.label(crate::ui::theme::section_heading("Repository names"));
    ui.label("Choose local display names for this project's registered repositories.");
    ui.add_space(8.0);
    repository_names::paint(ui, &mut dialog.repositories);
    ui.collapsing("Connect a secondary code repository", |ui| {
        ui.label("The selected checkout stays local. Task work uses separate isolated clones; published specifications remain on the primary repository.");
        ui.add(egui::TextEdit::singleline(&mut dialog.new_repository_id).hint_text("Stable ID (lowercase, digits, hyphens)"));
        ui.add(egui::TextEdit::singleline(&mut dialog.new_repository_role).hint_text("Role (for example, Backend API)"));
        ui.add(egui::TextEdit::singleline(&mut dialog.new_repository_remote).hint_text("Portable Git remote identity"));
        if ui.button("Choose local checkout…").clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            dialog.new_repository_checkout = Some(path);
        }
        if let Some(path) = &dialog.new_repository_checkout {
            ui.label(path.display().to_string());
        }
        if ui.button("Register secondary repository").clicked() {
            dialog.register_repository_requested = true;
        }
    });
    agent_guidance::paint(ui, dialog);
    ui.add_space(12.0);
    ui.label(crate::ui::theme::section_heading("Harness routes"));
    ui.label("Choose project-wide provider routes by role. Task overrides take precedence, followed by these project routes and then Coding tools defaults.");
    let settings = crate::persistence::harness_settings::load().0;
    paint_routes(ui, &mut dialog.harness_routes, &settings);
    ui.add_space(6.0);
    crate::app::dialogs::footers(ui, &dialog.feedback)
}
