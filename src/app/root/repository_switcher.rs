use super::{PacketApp, Screen};
use crate::ui::RepositoryChoice;

#[cfg(test)]
mod tests;

pub(super) fn choices(screen: &Screen) -> Vec<RepositoryChoice> {
    let Screen::Connected(project) = screen else {
        return Vec::new();
    };
    let repositories = &project.state.repositories.repositories;
    let labels = crate::core::project_repos::display_labels(repositories);
    repositories
        .iter()
        .zip(labels)
        .map(|(repository, label)| RepositoryChoice {
            id: repository.id.clone(),
            label,
        })
        .collect()
}

impl PacketApp {
    pub(super) fn open_registered_repository(&mut self, id: &str) {
        let target = match &self.screen {
            Screen::Connected(project) => project
                .state
                .repositories
                .target_if_available(&project.state.repo_root, id),
            Screen::Welcome => return,
        };
        let path = match target {
            Ok(Some(path)) => path,
            Ok(None) => {
                self.toasts.warning(format!(
                    "No local checkout is mapped for repository {id}; map a checkout before opening it."
                ));
                return;
            }
            Err(error) => {
                self.toasts
                    .warning(format!("Could not resolve repository {id}: {error}"));
                return;
            }
        };
        let binary = self
            .spawn_target_override
            .clone()
            .map(Ok)
            .unwrap_or_else(crate::app::spawn::resolve_self_executable);
        match binary.and_then(|binary| crate::app::spawn::spawn_sibling_in(&binary, &path)) {
            Ok(()) => self
                .toasts
                .info(format!("Opening {} in a new Packet window", path.display())),
            Err(error) => self.toasts.warning(error),
        }
    }
}
