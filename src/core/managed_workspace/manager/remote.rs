use anyhow::Context;
use std::{fs, path::Path};

use super::{ManagedWorkspace, WorkspaceManager};

impl WorkspaceManager {
    pub fn planning_remote_url(&self, id: uuid::Uuid) -> anyhow::Result<Option<String>> {
        let workspace = self.open(id)?;
        super::super::git::origin(&workspace.planning_repo)
            .map(|remote| validate_remote(&remote))
            .transpose()
    }

    /// Set the planning repository's shared Git remote after rejecting values
    /// that could embed credentials or alter the process command line.
    pub fn set_planning_remote(
        &self,
        id: uuid::Uuid,
        remote: &str,
    ) -> anyhow::Result<ManagedWorkspace> {
        let remote = validate_remote(remote)?;
        let workspace = self.open(id)?;
        crate::core::planning_sync::configure_remote(
            &workspace.planning_repo,
            &self.home,
            &remote,
        )?;
        self.open(id)
    }

    /// Clone and join a remote planning repository into this operator's
    /// managed workspace area. The remote manifest supplies the stable ID.
    pub fn join_remote(&self, remote: &str) -> anyhow::Result<ManagedWorkspace> {
        let remote = validate_remote(remote)?;
        let projects = self.home.join("projects");
        fs::create_dir_all(&projects)?;
        let staging = projects.join(format!(".join-{}", uuid::Uuid::new_v4().simple()));
        let checkout = staging.join("planning-repo");
        fs::create_dir(&staging)?;
        let cloned = super::super::git::run(
            None,
            &[
                "clone",
                "--no-tags",
                "--",
                remote.as_str(),
                &checkout.to_string_lossy(),
            ],
        );
        if let Err(error) = cloned {
            let _ = fs::remove_dir_all(&staging);
            return Err(error.context("planning repository could not be cloned"));
        }
        let result = join_cloned(self, remote.as_str(), &staging, &checkout);
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }
}

fn join_cloned(
    manager: &WorkspaceManager,
    remote: &str,
    staging: &Path,
    checkout: &Path,
) -> anyhow::Result<ManagedWorkspace> {
    let manifest = super::super::manifest::WorkspaceManifest::read_unvalidated(checkout)?;
    let canonical = manifest.canonical_ref.as_str();
    let valid = super::super::git::output(Some(checkout), &["check-ref-format", canonical])?;
    anyhow::ensure!(
        canonical.starts_with("refs/heads/") && valid.status.success(),
        "planning manifest canonical ref is invalid"
    );
    let fetched =
        super::super::git::run(Some(checkout), &["fetch", "--no-tags", "origin", canonical])?;
    let _ = fetched;
    let branch = canonical.strip_prefix("refs/heads/").unwrap();
    super::super::git::run(
        Some(checkout),
        &["checkout", "--force", "-B", branch, "FETCH_HEAD"],
    )?;
    let validated = super::super::manifest::WorkspaceManifest::read(checkout)?;
    anyhow::ensure!(
        validated.workspace_id == manifest.workspace_id,
        "planning workspace identity changed while joining"
    );
    let id = validated.workspace_id;
    let staged_store = crate::artifacts::planning_store::PlanningStore::new(
        id,
        checkout,
        crate::artifacts::planning_store::StoreMode::ManagedShared,
    );
    crate::core::project_repos::ProjectManifest::load(&staged_store)
        .context("planning project repository manifest is invalid")?;
    let requested_identity = crate::core::planning_sync::remote_identity_for(checkout, remote);
    let _join_lock = super::super::locking::lock(&manager.home.join("projects/.join.lock"))?;
    for local in manager.list_local()? {
        if local.id == id {
            continue;
        }
        if let Some(existing_remote) = manager.planning_remote_url(local.id)? {
            let existing_identity = crate::core::planning_sync::remote_identity_for(
                &local.planning_repo,
                &existing_remote,
            );
            anyhow::ensure!(
                existing_identity != requested_identity,
                "planning remote is already associated locally with workspace {}; its manifest declares workspace {id}; join was cancelled to prevent splitting workspace identity",
                local.id
            );
        }
    }
    if manager.association_path(id).is_file() {
        let existing = manager.open(id)?;
        let existing_remote = super::super::git::origin(&existing.planning_repo)
            .ok_or_else(|| anyhow::anyhow!("associated workspace has no planning remote"))?;
        let existing_identity = crate::core::planning_sync::remote_identity_for(
            &existing.planning_repo,
            &existing_remote,
        );
        anyhow::ensure!(
            existing_identity == requested_identity,
            "workspace {id} is already associated with a different planning remote"
        );
        fs::remove_dir_all(staging)?;
        return Ok(existing);
    }
    let project_dir = manager.project_dir(id);
    let project_dir_existed = project_dir.exists();
    if project_dir.exists() {
        anyhow::ensure!(
            fs::read_dir(&project_dir)?.next().is_none(),
            "workspace {id} already has local data; the existing directory was not replaced"
        );
    } else {
        fs::create_dir_all(&project_dir)?;
    }
    let destination = manager.planning_repo_path(id);
    anyhow::ensure!(
        !destination.exists(),
        "planning repository destination already exists; it was not overwritten"
    );
    fs::rename(checkout, &destination)?;
    let _ = fs::remove_dir(staging);
    match manager.join_while_locked(&destination) {
        Ok(workspace) => Ok(workspace),
        Err(error) => {
            if let Err(cleanup) =
                rollback_remote_join(manager, id, &destination, project_dir_existed)
            {
                return Err(error.context(format!("remote join rollback failed: {cleanup:#}")));
            }
            Err(error.context("remote planning repository was moved but could not be associated; the move was rolled back"))
        }
    }
}

fn rollback_remote_join(
    manager: &WorkspaceManager,
    id: uuid::Uuid,
    destination: &Path,
    project_dir_existed: bool,
) -> anyhow::Result<()> {
    let project_dir = manager.project_dir(id);
    let association = manager.association_path(id);
    if association.exists() {
        fs::remove_file(association)?;
    }
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    let workspace_lock = project_dir.join(".workspace.lock");
    if workspace_lock.exists() {
        fs::remove_file(workspace_lock)?;
    }
    if !project_dir_existed && project_dir.exists() && fs::read_dir(&project_dir)?.next().is_none()
    {
        fs::remove_dir(project_dir)?;
    }
    Ok(())
}

fn validate_remote(raw: &str) -> anyhow::Result<String> {
    let remote = raw.trim();
    anyhow::ensure!(!remote.is_empty(), "planning remote URL is required");
    anyhow::ensure!(
        remote
            .chars()
            .all(|character| !character.is_control() && !character.is_whitespace()),
        "planning remote URL cannot contain whitespace or control characters"
    );
    if let Ok(url) = url::Url::parse(remote) {
        let has_userinfo = !url.username().is_empty() || url.password().is_some();
        let http_remote = matches!(url.scheme(), "http" | "https");
        let http_userinfo = http_remote && has_userinfo;
        anyhow::ensure!(
            !http_userinfo && url.password().is_none(),
            "planning remote URL must not embed credentials"
        );
        anyhow::ensure!(
            !http_remote || (url.query().is_none() && url.fragment().is_none()),
            "planning HTTP remote URL cannot contain a query or fragment"
        );
    }
    Ok(remote.to_owned())
}
