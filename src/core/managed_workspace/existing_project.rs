//! Read-only inspection and explicitly approved attachment of local code.
use super::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager, git};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingProjectKind {
    Tracked,
    NeedsHistory,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExistingProject {
    pub folder: PathBuf,
    #[serde(default)]
    pub planning_repo: Option<PathBuf>,
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
        self.attach_existing_project_with_planning_repo(
            request,
            request.planning_repo.as_deref(),
            approve_git_init,
        )
    }

    /// Attach an existing code checkout to a selected local planning
    /// repository. The planning checkout is joined by its own manifest ID;
    /// the UI's draft workspace ID is never treated as authority.
    pub fn attach_existing_project_with_planning_repo(
        &self,
        request: &ExistingProject,
        selected_planning_repo: Option<&Path>,
        approve_git_init: bool,
    ) -> anyhow::Result<ManagedWorkspace> {
        let kind = self.inspect_existing_project(&request.folder)?;
        let existing_workspace = if request.create_workspace {
            None
        } else {
            selected_planning_repo
                .is_none()
                .then(|| self.open(request.workspace_id))
                .transpose()?
        };
        let planning = if request.create_workspace {
            super::application::preflight::canonical_location(
                &self.planning_repo_path(request.workspace_id),
            )?
        } else if let Some(path) = selected_planning_repo {
            self.inspect_planning_repository(path)?;
            path.canonicalize()?
        } else {
            existing_workspace
                .as_ref()
                .expect("selected workspace was opened")
                .planning_repo
                .clone()
        };
        anyhow::ensure!(
            !request.folder.starts_with(&planning) && !planning.starts_with(&request.folder),
            "selected planning history and application code must not be nested"
        );

        if kind == ExistingProjectKind::NeedsHistory {
            anyhow::ensure!(
                approve_git_init,
                "approve turning on change history for this exact folder first"
            );
            git::init_planning_repo(&request.folder)?;
        }
        let workspace = if request.create_workspace {
            self.create_identified(request.workspace_id, &request.title)?;
            self.open(request.workspace_id)?
        } else if let Some(path) = selected_planning_repo {
            self.join(path)?
        } else {
            existing_workspace.expect("selected workspace was opened")
        };
        let existing_checkout_id = workspace
            .association
            .code_checkouts
            .iter()
            .find(|checkout| checkout.path == request.folder)
            .map(|checkout| checkout.repository_id.clone());
        let sibling_planning_repo = request
            .folder
            .parent()
            .zip(request.folder.file_name())
            .map(|(parent, name)| parent.join(format!("{}.packet", name.to_string_lossy())))
            .and_then(|path| path.canonicalize().ok())
            .is_some_and(|path| path == workspace.planning_repo);
        let code_remote = git::origin(&request.folder).unwrap_or_default();
        let pair_root_id = sibling_planning_repo
            .then(|| {
                workspace
                    .repositories
                    .repositories
                    .iter()
                    .find(|repository| {
                        repository.id == super::PAIR_CODE_REPO_ID
                            && (repository.remote.is_empty() || repository.remote == code_remote)
                    })
                    .map(|repository| repository.id.clone())
            })
            .flatten();
        if let Some(workspace) = self.map_existing_checkout_if_available(
            workspace.id,
            existing_checkout_id.as_deref().or(pair_root_id.as_deref()),
            &request.folder,
        )? {
            return Ok(workspace);
        }
        self.register_existing_checkout_if_available(
            workspace.id,
            CodeRepositoryRegistration {
                id: request.repository_id.clone(),
                role: "application".into(),
                remote: None,
                checkout: request.folder.clone(),
            },
        )?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "this project request is already linked to a different local checkout; choose the code folder again to create a new request"
            )
        })
    }
}
