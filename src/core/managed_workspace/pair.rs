//! Create a visible code + planning sibling pair for a new project.
//!
//! The wizard's "create new workspace" track materializes two ordinary
//! Git repositories chosen by the operator, side by side under a parent
//! folder: `Parent/SomeRepo` (code) and `Parent/SomeRepo.packet` (planning).
//! The planning repository is seeded exactly like a managed workspace and is
//! then associated through the regular join path, so it appears in every
//! operator view with the usual identity handling.
//!
//! Retrying is safe: already-initialized repositories are recognized and
//! reused instead of rebuilt.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    CodeRepositoryRegistration, WorkspaceManager,
    creation::{self, initialize},
    git,
    manager::{ManagedWorkspace, sync_directory},
    manifest::WorkspaceManifest,
};

pub const PAIR_CODE_REPO_ID: &str = "root";

/// Inputs for creating a new sibling repository pair.
pub struct SiblingPairSpec {
    pub title: String,
    pub code_repo_name: String,
    pub planning_repo_name: String,
    pub parent_dir: PathBuf,
}

impl SiblingPairSpec {
    /// Builds a spec from a project name, suggesting `<name>.packet` for the
    /// planning repository (operators may rename it afterwards).
    pub fn new(title: impl Into<String>, code_repo_name: impl Into<String>) -> Self {
        let code_repo_name = code_repo_name.into();
        Self {
            title: title.into(),
            planning_repo_name: format!("{}.packet", code_repo_name),
            code_repo_name,
            parent_dir: PathBuf::new(),
        }
    }
}

pub(super) fn create_sibling_pair(
    manager: &WorkspaceManager,
    spec: &SiblingPairSpec,
) -> anyhow::Result<ManagedWorkspace> {
    creation::validate_title(&spec.title)?;
    validate_repo_name(&spec.code_repo_name, "code repository name")?;
    validate_repo_name(&spec.planning_repo_name, "planning repository name")?;
    let parent = spec.parent_dir.canonicalize().map_err(|error| {
        anyhow::anyhow!(
            "workspace parent folder is missing or unreadable (select an existing folder): {error}"
        )
    })?;
    anyhow::ensure!(parent.is_dir(), "workspace parent must be a folder");
    let code_dest = parent.join(&spec.code_repo_name);
    let plan_dest = parent.join(&spec.planning_repo_name);
    anyhow::ensure!(
        code_dest != plan_dest,
        "code and planning repository names must differ"
    );

    let code = prepare_code_repository(&spec.title, &code_dest)?;
    prepare_planning_repository(spec, &parent, &plan_dest)?;
    let workspace = manager.join(&plan_dest)?;
    register_pair_code_repository(manager, &workspace, &code)
}

fn prepare_code_repository(title: &str, code_dest: &Path) -> anyhow::Result<PathBuf> {
    let existing = is_git_repository_root(code_dest);
    if code_dest.exists() && !existing {
        anyhow::bail!(
            "{} already exists and is not a Git repository root",
            code_dest.display()
        );
    }
    if !existing {
        let name = code_dest
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("code repository path is not valid UTF-8"))?;
        git::run(
            None,
            &[
                "-c",
                "init.templateDir=",
                "init",
                "--quiet",
                "--initial-branch=main",
                "--",
                name,
            ],
        )
        .map_err(|error| {
            let _ = fs::remove_dir_all(code_dest);
            anyhow::anyhow!("code repository could not be initialized at {name}: {error}")
        })?;
    }
    if !git::has_commit(code_dest)? {
        let readme = format!("# {title}\n\nCode repository maintained by Kool.ad/e.\n");
        let write_outcome = fs::write(code_dest.join("README.md"), readme);
        if let Err(write_error) = write_outcome {
            let _ = fs::remove_dir_all(code_dest);
            anyhow::bail!(
                "code repository initial README could not be written (the unfinished repository was removed): {write_error}"
            );
        }
        let commit_outcome = crate::core::gitops::commit(
            code_dest,
            &format!("Kool.ad/e: initialize {title} code repository"),
            &["README.md".to_owned()],
        );
        if let Err(commit_error) = commit_outcome {
            let _ = fs::remove_dir_all(code_dest);
            anyhow::bail!(
                "code repository initial commit failed (the unfinished repository was removed): {commit_error}"
            );
        }
    }
    Ok(code_dest.to_path_buf())
}

fn prepare_planning_repository(
    spec: &SiblingPairSpec,
    parent: &Path,
    plan_dest: &Path,
) -> anyhow::Result<()> {
    if is_git_repository_root(plan_dest) {
        let manifest = WorkspaceManifest::read(plan_dest).map_err(|error| {
            anyhow::anyhow!(
                "{} is a Git repository but its planning manifest is unreadable: {error}",
                plan_dest.display()
            )
        })?;
        let _ = manifest;
        anyhow::ensure!(
            git::status_paths(plan_dest)?.is_empty(),
            "{} must be clean before it can be reused as the planning repository",
            plan_dest.display()
        );
        return Ok(());
    }
    if plan_dest.exists() {
        anyhow::bail!(
            "{} already exists and is not a usable planning repository",
            plan_dest.display()
        );
    }
    let id = uuid::Uuid::new_v4();
    let staging = parent.join(format!(
        "{}.__staging__{}",
        spec.planning_repo_name,
        id.simple()
    ));
    build_planning_staging(&staging, id, &spec.title)?;
    anyhow::ensure!(
        !plan_dest.exists(),
        "{} appeared during creation; it was not overwritten",
        plan_dest.display()
    );
    fs::rename(&staging, plan_dest).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        anyhow::anyhow!("planning repository could not be moved into place: {error}")
    })?;
    sync_directory(parent)
        .map_err(|error| anyhow::anyhow!("planning repository was not flushed to disk: {error}"))
}

fn build_planning_staging(staging: &Path, id: uuid::Uuid, title: &str) -> anyhow::Result<()> {
    let cleanup_on_error = || {
        let _ = fs::remove_dir_all(staging);
    };
    let result = (|| -> anyhow::Result<()> {
        git::init_planning_repo(staging)?;
        initialize::populate_and_commit(staging, id, title)
    })();
    if result.is_err() {
        cleanup_on_error();
    }
    result?;
    let flush = initialize::sync_tree(staging);
    if flush.is_err() {
        cleanup_on_error();
    }
    flush.map_err(|error| anyhow::anyhow!("planning repository staging was not flushed: {error}"))
}

fn register_pair_code_repository(
    manager: &WorkspaceManager,
    workspace: &ManagedWorkspace,
    code_dest: &Path,
) -> anyhow::Result<ManagedWorkspace> {
    let existing = workspace
        .repositories
        .repositories
        .iter()
        .find(|repository| repository.id == PAIR_CODE_REPO_ID);
    let mapped_same = workspace
        .association
        .code_checkouts
        .iter()
        .any(|mapping| mapping.repository_id == PAIR_CODE_REPO_ID && mapping.path == code_dest);
    if mapped_same {
        return Ok(workspace.clone());
    }
    if existing.is_some() {
        manager.disconnect_checkout(workspace.id, PAIR_CODE_REPO_ID)?;
    }
    manager.register_code_repository(
        workspace.id,
        CodeRepositoryRegistration {
            id: PAIR_CODE_REPO_ID.to_owned(),
            role: "Primary code".to_owned(),
            remote: None,
            checkout: code_dest.to_path_buf(),
        },
    )
}

fn is_git_repository_root(path: &Path) -> bool {
    git::repository_root(path).is_ok_and(|root| root == path)
}

fn validate_repo_name(name: &str, what: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=64).contains(&name.len())
            && name
                .bytes()
                .next()
                .is_some_and(|byte: u8| byte.is_ascii_alphabetic()),
        "{what} must start with a letter and be at most 64 characters"
    );
    anyhow::ensure!(
        name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        }),
        "{what} may contain letters, digits, dots, underscores, and dashes only"
    );
    anyhow::ensure!(
        !name.starts_with('.') && name != "." && name != "..",
        "{what} cannot start with a dot or be a traversal name"
    );
    Ok(())
}
