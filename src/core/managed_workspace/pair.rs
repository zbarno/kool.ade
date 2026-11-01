//! Create code in the selected parent and planning history in Kool.ad/e's
//! managed workspace area.
//!
//! The wizard's "create new workspace" track materializes a fresh code
//! repository at the selected parent and a planning repository in the managed
//! workspace area.
//!
//! Reopening is only safe through a saved workspace association. Unassociated
//! destinations are never adopted, even when they look like Kool.ad/e output.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    CodeRepositoryRegistration, WorkspaceManager, creation, git, manager::ManagedWorkspace,
};

pub const PAIR_CODE_REPO_ID: &str = "root";

/// Inputs for creating a new code repository and managed planning workspace.
pub struct NewProjectSpec {
    pub title: String,
    pub code_repo_name: String,
    pub parent_dir: PathBuf,
}

impl NewProjectSpec {
    /// Builds a spec with a derived code-repository directory name.
    pub fn new(title: impl Into<String>, code_repo_name: impl Into<String>) -> Self {
        let code_repo_name = code_repo_name.into();
        Self {
            title: title.into(),
            code_repo_name,
            parent_dir: PathBuf::new(),
        }
    }
}

pub(super) fn create_new_project(
    manager: &WorkspaceManager,
    spec: &NewProjectSpec,
) -> anyhow::Result<ManagedWorkspace> {
    creation::validate_title(&spec.title)?;
    validate_repo_name(&spec.code_repo_name, "code repository name")?;
    let parent = spec.parent_dir.canonicalize().map_err(|error| {
        anyhow::anyhow!(
            "workspace parent folder is missing or unreadable (select an existing folder): {error}"
        )
    })?;
    anyhow::ensure!(parent.is_dir(), "workspace parent must be a folder");
    let code_dest = parent.join(&spec.code_repo_name);
    match fs::symlink_metadata(&code_dest) {
        Ok(metadata) => {
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "code repository destination is a symbolic link; choose a new project name"
            );
            if let Some(workspace) = manager.workspace_for_checkout(&code_dest)? {
                let canonical_destination = code_dest.canonicalize()?;
                let registered = workspace.association.code_checkouts.iter().any(|mapping| {
                    mapping.repository_id == PAIR_CODE_REPO_ID
                        && mapping.path == canonical_destination
                });
                anyhow::ensure!(
                    registered,
                    "code repository destination already exists but is not registered as this workspace's primary code checkout"
                );
                return Ok(workspace);
            }
            anyhow::bail!(
                "code repository destination {} already exists; Kool.ad/e will not adopt an existing checkout. Choose a new project name or open its saved workspace.",
                code_dest.display()
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let staging = parent.join(format!(
        ".{}.koolade-create-{}",
        spec.code_repo_name,
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&staging).map_err(|error| {
        anyhow::anyhow!("exclusive project staging directory could not be reserved: {error}")
    })?;
    let staged = (|| {
        git::init_planning_repo(&staging)?;
        ground_initial_commit(&staging, &spec.title)?;
        creation::sync_tree(&staging)?;
        Ok::<(), anyhow::Error>(())
    })();
    staged.map_err(|error| {
        anyhow::anyhow!(
            "{error:#}; Kool.ad/e left its incomplete staging folder at {}",
            staging.display()
        )
    })?;
    super::application::publish::directory(&staging, &code_dest).map_err(|error| {
        anyhow::anyhow!(
            "{error:#}; Kool.ad/e left the new checkout staged at {}",
            staging.display()
        )
    })?;
    super::manager::sync_directory(&parent)?;

    let workspace = manager.create(&spec.title)?;
    register_pair_code_repository(manager, &workspace, &code_dest)
}

fn ground_initial_commit(code_dest: &Path, title: &str) -> anyhow::Result<()> {
    let readme = format!("# {title}\n\nCode repository maintained by Kool.ad/e.\n");
    let readme_path = code_dest.join("README.md");
    fs::write(&readme_path, readme).map_err(|error| {
        anyhow::anyhow!("code repository placeholder README could not be written: {error}")
    })?;
    let commit = crate::core::gitops::commit(
        code_dest,
        &format!("Kool.ad/e: initialize {title} code repository"),
        &["README.md".to_owned()],
    )
    .map(|_| ());
    commit.map_err(|error| {
        let _ = fs::remove_file(&readme_path);
        anyhow::anyhow!("code repository initial commit failed: {error}")
    })
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

/// The COMPLETE pure rule behind `validate_repo_name` — first character
/// alphabetic, length 1 to 64, restricted charset, no leading dot or
/// traversal name — exposed so UI layers can refuse unusable names at the
/// input boundary instead of carrying them through a multi-second create
/// flow into this check. Keep in sync with `validate_repo_name`.
pub(crate) fn repository_name_allowed(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .next()
            .is_some_and(|byte: u8| byte.is_ascii_alphabetic())
        && !name.starts_with('.')
        && name != "."
        && name != ".."
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
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

#[cfg(test)]
mod name_rule_tests {
    use super::{repository_name_allowed, validate_repo_name};

    #[test]
    fn exposed_rule_cannot_drift_from_validate_repo_name() {
        let corpus: Vec<String> = vec![
            "".into(),
            "a".into(),
            "zorg".into(),
            "Demo2".into(),
            "payroll-service".into(),
            "demo.x_y-z".into(),
            "a.".into(),
            "a-b".into(),
            "_.x".into(),
            "1abc".into(),
            "-abc".into(),
            ".hidden".into(),
            ".".into(),
            "..".into(),
            "a\u{2014}dash".into(),
            "x".repeat(64),
            "x".repeat(65),
            "segment".repeat(10),
        ];
        for name in corpus {
            let allowed = repository_name_allowed(&name);
            let validated = validate_repo_name(&name, "corpus name").is_ok();
            assert_eq!(
                allowed, validated,
                "predicate and validator disagree on {name:?}"
            );
        }
    }
}
