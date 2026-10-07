use super::{Project, RepositoryNameRow, repository_names};
use crate::{AppError, core::gitops};

pub struct DlgProjectSettings {
    pub repositories: Vec<RepositoryNameRow>,
    pub feedback: Option<(bool, String)>,
}

impl DlgProjectSettings {
    pub fn from_project(project: &Project) -> Self {
        Self {
            repositories: repository_names::rows(&project.state.repositories),
            feedback: None,
        }
    }

    pub fn apply(&mut self, project: &mut Project) -> Result<Option<String>, AppError> {
        let _guard = crate::core::writer_gate::acquire();
        if !repository_names::persist(project, &mut self.repositories)? {
            return Ok(None);
        }
        project.state.repositories =
            crate::core::project_repos::ProjectManifest::load(&project.state.repo_root)
                .map_err(AppError::from)?;
        let path = crate::artifacts::layout::canonical::PROJECT_MANIFEST.to_string();
        let sha = gitops::commit(
            &project.state.repo_root,
            "settings: update project repositories",
            &[path],
        )?;
        project.refresh_git();
        Ok(Some(sha.chars().take(7).collect()))
    }
}

pub fn paint_project_settings_card(
    ui: &mut egui::Ui,
    dialog: &mut DlgProjectSettings,
) -> (bool, bool) {
    ui.heading("Project / Git");
    ui.label("Names shown for this project's registered repositories. Branch and worktree defaults can be configured here as those controls are added.");
    ui.add_space(8.0);
    repository_names::paint(ui, &mut dialog.repositories);
    ui.add_space(6.0);
    crate::app::dialogs::footers(ui, &dialog.feedback)
}
