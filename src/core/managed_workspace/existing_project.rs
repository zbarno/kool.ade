//! Read-only inspection and explicitly approved attachment of local code.
use super::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager, git};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingProjectKind {
    Tracked,
    NeedsHistory,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExistingProject {
    pub folder: PathBuf,
    pub workspace_id: uuid::Uuid,
    pub repository_id: String,
    pub title: String,
    pub create_workspace: bool,
}

impl WorkspaceManager {
    pub fn inspect_existing_project(&self, folder: &Path) -> anyhow::Result<ExistingProjectKind> {
        anyhow::ensure!(
            folder.is_dir() && folder.canonicalize()? == folder,
            "choose an existing folder without redirected path components"
        );
        let state_root = super::application::preflight::canonical_location(&self.home)?;
        anyhow::ensure!(
            !folder.starts_with(&state_root) && !state_root.starts_with(folder),
            "application code and planning history must be separate"
        );
        anyhow::ensure!(
            !folder.join(".koolade-packet").exists() && !folder.join(".kool-ade-packet").exists(),
            "this project has planning history; use Advanced to preview migration or join it"
        );
        match git::repository_root(folder) {
            Ok(root) => {
                anyhow::ensure!(
                    root.canonicalize()? == folder,
                    "this folder is inside another repository; choose its top-level folder"
                );
                let superproject = git::run(
                    Some(folder),
                    &["rev-parse", "--show-superproject-working-tree"],
                )?;
                anyhow::ensure!(
                    superproject.stdout.trim_ascii().is_empty(),
                    "this is a submodule; choose the containing project or use Advanced"
                );
                Ok(ExistingProjectKind::Tracked)
            }
            Err(_) => {
                // This rejects ambiguous Git/permission/configuration errors,
                // bare repositories and ancestor .git markers rather than init.
                super::application::preflight::outside_repository(folder)?;
                Ok(ExistingProjectKind::NeedsHistory)
            }
        }
    }

    /// Consent is invocation-local and must be obtained again after restart.
    /// Existing repositories receive no add, commit, checkout, config or reset.
    pub fn attach_existing_project(
        &self,
        request: &ExistingProject,
        approve_git_init: bool,
    ) -> anyhow::Result<ManagedWorkspace> {
        let kind = self.inspect_existing_project(&request.folder)?;
        let planning = if request.create_workspace {
            super::application::preflight::canonical_location(
                &self.planning_repo_path(request.workspace_id),
            )?
        } else {
            super::WorkspaceAssociation::read(
                &self.association_path(request.workspace_id),
                request.workspace_id,
            )?
            .planning_repo_path
            .canonicalize()?
        };
        anyhow::ensure!(
            !request.folder.starts_with(&planning) && !planning.starts_with(&request.folder),
            "selected planning history and application code must not be nested"
        );
        if !request.create_workspace {
            self.open(request.workspace_id)?;
        }

        if kind == ExistingProjectKind::NeedsHistory {
            anyhow::ensure!(
                approve_git_init,
                "approve turning on change history for this exact folder first"
            );
            git::init_planning_repo(&request.folder)?;
        }
        if request.create_workspace {
            self.create_identified(request.workspace_id, &request.title)?;
        } else {
            self.open(request.workspace_id)?;
        }
        self.register_code_repository(
            request.workspace_id,
            CodeRepositoryRegistration {
                id: request.repository_id.clone(),
                role: "application".into(),
                remote: None,
                checkout: request.folder.clone(),
            },
        )
    }
}
