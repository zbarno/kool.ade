use super::{KooladeApp, Screen};
use crate::{
    artifacts::planning_store::StoreMode,
    ui::{PlanningCodeRepositoryView, PlanningRepositoryViewModel},
};

impl KooladeApp {
    pub(super) fn planning_repository_view(&self) -> Option<PlanningRepositoryViewModel> {
        let Screen::Connected(project) = &self.screen else {
            return None;
        };
        let managed = project.managed_workspace.as_ref();
        let workspace_id = managed
            .map(|workspace| workspace.id.hyphenated().to_string())
            .unwrap_or_else(|| "legacy-local-project".into());
        let repositories = if let Some(workspace) = managed {
            project
                .state
                .repositories
                .repositories
                .iter()
                .map(|repository| {
                    let checkout = workspace
                        .association
                        .code_checkouts
                        .iter()
                        .find(|entry| entry.repository_id == repository.id)
                        .map(|entry| entry.path.clone());
                    PlanningCodeRepositoryView {
                        id: repository.id.clone(),
                        remote: repository.remote.clone(),
                        checkout,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        Some(PlanningRepositoryViewModel {
            workspace_id,
            local_path: project.state.planning_store.root.clone(),
            shared: project.state.planning_store.mode == StoreMode::ManagedShared,
            canonical_ref: managed
                .map(|workspace| workspace.manifest.canonical_ref.clone())
                .unwrap_or_default(),
            remote_url: project.planning_remote_url.clone(),
            repositories,
            sync: project.planning_sync.clone(),
            sync_running: self.planning_sync_job.is_some(),
            migration_available: project.state.planning_store.mode == StoreMode::LegacyPacketSource,
        })
    }

    pub(super) fn update_planning_remote(&mut self, remote: &str) -> Result<(), String> {
        let Screen::Connected(project) = &mut self.screen else {
            return Err("Connect a workspace before configuring its planning remote.".into());
        };
        if project.state.planning_store.mode == StoreMode::LegacyPacketSource {
            return Err("Migrate this legacy project before configuring a planning remote.".into());
        }
        let id = project.state.planning_store.project_id;
        let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
        let workspace = manager
            .set_planning_remote(id, remote)
            .map_err(|error| error.to_string())?;
        project.state.planning_store = workspace.store.clone();
        project.managed_workspace = Some(workspace);
        project.planning_remote_url = Some(remote.trim().to_owned());
        project.planning_sync = manager
            .planning_sync_status(id)
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) fn build_migration_preview(
        &self,
    ) -> Result<crate::core::managed_workspace::MigrationPreview, String> {
        let Screen::Connected(project) = &self.screen else {
            return Err("Connect a legacy project before previewing migration.".into());
        };
        if project.state.planning_store.mode != StoreMode::LegacyPacketSource {
            return Err("This workspace already uses a managed planning repository.".into());
        }
        crate::core::managed_workspace::WorkspaceManager::from_state_root()
            .preview_legacy_migration(&project.state.repo_root)
            .map_err(|error| error.to_string())
    }

    pub(super) fn confirm_legacy_migration(
        &mut self,
        preview: &crate::core::managed_workspace::MigrationPreview,
    ) -> Result<(), String> {
        let Screen::Connected(project) = &self.screen else {
            return Err("Connect a legacy project before migrating it.".into());
        };
        if project.state.planning_store.mode != StoreMode::LegacyPacketSource {
            return Err("This workspace already uses a managed planning repository.".into());
        }
        if project.active_turn.is_some() || !project.active_implementations.is_empty() {
            return Err("Finish active planning and implementation work before migration.".into());
        }
        let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
        manager
            .migrate_legacy_checkout_from_preview(preview)
            .map_err(|error| error.to_string())?;
        let selected = preview.source_root.to_string_lossy().into_owned();
        self.finish_connect(crate::app::welcome::attempt_connect(&selected));
        Ok(())
    }
}
