use std::path::PathBuf;

mod github;
mod recovery;
mod workspace;

use crate::core::managed_workspace::WorkspaceManager;

use super::github::{
    SetupLock, clear_pending_clone_receipt, clone_destination, parse_github_url,
    pending_clone_workspace_id, perform_clone_for_workspace,
};
use github::{create_remote_planning_repository, repository_visibility};
use recovery::resume_interrupted_planning_push;
use workspace::{
    managed_code_checkout, missing_planning_workspace, register_code_checkout,
    reject_unassociated_clone_destination, reusable_local_pair, reusable_pending_clone,
    same_github_remote, validate_planning_name,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepositoryVisibility {
    Public,
    Private,
    Internal,
}

impl RepositoryVisibility {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.trim() {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            "internal" => Ok(Self::Internal),
            other => anyhow::bail!("GitHub returned an unsupported repository visibility: {other}"),
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Self::Public => "--public",
            Self::Private => "--private",
            Self::Internal => "--internal",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PairedRepositoryPreview {
    pub(crate) owner: String,
    pub(crate) code_name: String,
    pub(crate) planning_name: String,
    pub(crate) code_visibility: RepositoryVisibility,
    pub(crate) planning_visibility: Option<RepositoryVisibility>,
}

pub(crate) fn inspect_github_pair(raw: &str) -> anyhow::Result<PairedRepositoryPreview> {
    let target = parse_github_url(raw).map_err(anyhow::Error::msg)?;
    let planning_name = format!("{}.packet", target.repo);
    validate_planning_name(&planning_name)?;
    let code_visibility = repository_visibility(&target.owner, &target.repo)?
        .ok_or_else(|| anyhow::anyhow!("GitHub could not find the code repository."))?;
    let planning_visibility = repository_visibility(&target.owner, &planning_name)?;
    validate_pair_visibility(
        &target.owner,
        &planning_name,
        code_visibility,
        planning_visibility,
    )?;
    Ok(PairedRepositoryPreview {
        owner: target.owner,
        code_name: target.repo,
        planning_name,
        code_visibility,
        planning_visibility,
    })
}

fn validate_pair_visibility(
    owner: &str,
    planning_name: &str,
    code_visibility: RepositoryVisibility,
    planning_visibility: Option<RepositoryVisibility>,
) -> anyhow::Result<()> {
    if let Some(planning_visibility) = planning_visibility
        && planning_visibility != code_visibility
    {
        anyhow::bail!(
            "The planning repository {owner}/{planning_name} is {} while the code repository is {}. Kool.ad/e will not change either repository's visibility.",
            planning_visibility.label(),
            code_visibility.label()
        );
    }
    Ok(())
}

pub(crate) fn perform_clone_and_pair(raw: &str, title: &str) -> anyhow::Result<PathBuf> {
    let target = parse_github_url(raw).map_err(anyhow::Error::msg)?;
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Set $HOME to a writable directory before cloning."))?;
    let destination = clone_destination(&target.repo, &home);
    let _setup_lock = SetupLock::acquire(&destination)
        .map_err(|error| anyhow::anyhow!("could not lock managed clone setup: {error}"))?;
    let preview = inspect_github_pair(raw)?;
    let planning_remote = format!(
        "https://github.com/{}/{}.packet.git",
        target.owner, target.repo
    );
    let manager = WorkspaceManager::from_state_root();
    let pending_workspace_id =
        pending_clone_workspace_id(&target.url, &target.repo).map_err(anyhow::Error::msg)?;
    let registered = if preview.planning_visibility.is_none() {
        reusable_local_pair(&manager, &target.url)?
    } else {
        None
    };
    if preview.planning_visibility.is_none()
        && registered.is_none()
        && pending_workspace_id.is_none()
    {
        reject_unassociated_clone_destination(&target.repo)?;
    }
    let reusable = if preview.planning_visibility.is_none() {
        let pending = pending_workspace_id
            .map(|id| reusable_pending_clone(&manager, id, &target.url, &planning_remote))
            .transpose()?;
        match (registered, pending) {
            (Some(registered), Some(pending)) => {
                anyhow::ensure!(
                    registered.id == pending.id,
                    "The managed code repository and its interrupted setup point to different workspaces. Open the saved workspace and retry."
                );
                Some(registered)
            }
            (Some(registered), None) => Some(registered),
            (None, pending) => pending,
        }
    } else {
        None
    };
    let mut workspace = if preview.planning_visibility.is_some() {
        match manager.join_remote(&planning_remote) {
            Ok(workspace) => workspace,
            Err(join_error) => {
                match resume_interrupted_planning_push(&manager, &target.url, &planning_remote)? {
                    Some(workspace) => workspace,
                    None => {
                        return Err(join_error.context(
                        "existing planning repository could not be joined and no safe interrupted setup can be resumed",
                    ));
                    }
                }
            }
        }
    } else {
        missing_planning_workspace(&manager, reusable, title)?
    };
    let code_checkout = match managed_code_checkout(&manager, &workspace, &target.url)? {
        Some(checkout) => checkout,
        None => perform_clone_for_workspace(&target.url, &target.repo, workspace.id)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
    };
    let code_origin = crate::core::gitops::origin_url(&code_checkout)
        .ok_or_else(|| anyhow::anyhow!("the managed code checkout has no origin remote"))?;
    anyhow::ensure!(
        same_github_remote(&code_origin, &target.url),
        "The managed code checkout origin does not match the requested GitHub repository. Kool.ad/e left it untouched."
    );

    workspace = register_code_checkout(&manager, workspace, &code_checkout, &target.url)?;
    clear_pending_clone_receipt(&code_checkout, &target.url, workspace.id)?;
    if preview.planning_visibility.is_none() {
        create_remote_planning_repository(&preview, &planning_remote, &workspace.planning_repo)?;
        workspace = manager.set_planning_remote(workspace.id, &planning_remote)?;
    }
    anyhow::ensure!(
        workspace
            .repositories
            .repositories
            .iter()
            .any(|repository| same_github_remote(&repository.remote, &target.url)),
        "paired planning repository does not register the cloned code repository"
    );
    Ok(code_checkout)
}

#[cfg(test)]
#[path = "github_pair/tests.rs"]
mod tests;
