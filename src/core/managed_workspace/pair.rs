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
    /// When true the code destination must already be an initialized Git
    /// repository (ANY contents — the operator explicitly chose it) and is
    /// paired untouched instead of created.
    pub code_already_exists: bool,
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
            code_already_exists: false,
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
    // Classify BOTH destinations before mutating anything: an occupied
    // planning destination must not leave a freshly created code repository
    // behind (symmetric with the code-side refusal).
    classify_destination(&code_dest, "code repository")?;
    classify_destination(&plan_dest, "planning repository")?;

    let code = prepare_code_repository(&spec.title, &code_dest, spec.code_already_exists)?;
    prepare_planning_repository(spec, &parent, &plan_dest)?;
    let workspace = manager.join(&plan_dest)?;
    register_pair_code_repository(manager, &workspace, &code)
}

fn classify_destination(destination: &Path, kind: &str) -> anyhow::Result<()> {
    if !destination.exists() || is_git_repository_root(destination) {
        return Ok(());
    }
    anyhow::bail!(
        "{kind} destination {} already exists and is not a Git repository root; remove it or choose a different name",
        destination.display()
    )
}

fn prepare_code_repository(
    title: &str,
    code_dest: &Path,
    existing: bool,
) -> anyhow::Result<PathBuf> {
    if existing {
        // Explicit adoption: the operator pointed at THEIR repository. Any
        // history is accepted (this is not the silent-adoption path, which
        // applies only when a fresh creation collides with existing content),
        // and nothing inside it is mutated.
        anyhow::ensure!(
            is_git_repository_root(code_dest),
            "{} is not a Git repository root; choose a folder containing an initialized Git repository",
            code_dest.display()
        );
        return Ok(code_dest.to_path_buf());
    }
    if is_git_repository_root(code_dest) {
        // Existing repository: resume only when it is unmistakably ours.
        match code_repository_proof(code_dest, title)? {
            CodeRepositoryProof::Unborn => ground_initial_commit(code_dest, title)?,
            CodeRepositoryProof::Grounded => {}
        }
    } else {
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
        ground_initial_commit(code_dest, title)?;
    }
    Ok(code_dest.to_path_buf())
}

/// Provenance proof for reusing a repository found at the code destination:
/// anything Kool.ad/e did not demonstrably create is refused rather than
/// adopted, so a foreign (even clean) repository can never be silently
/// enrolled as the project's root code checkout.
#[derive(Debug, PartialEq, Eq)]
enum CodeRepositoryProof {
    /// No commits at all: either our own fresh init or an operator's empty
    /// repository — installing the grounding commit destroys nothing.
    Unborn,
    /// Exactly the grounding commit tracking exactly README.md with a clean
    /// tree: an earlier interrupted run of THIS creation, safe to resume.
    Grounded,
}

fn ground_commit_message(title: &str) -> String {
    format!("Kool.ad/e: initialize {title} code repository")
}

fn ground_initial_commit(code_dest: &Path, title: &str) -> anyhow::Result<()> {
    let readme = format!("# {title}\n\nCode repository maintained by Kool.ad/e.\n");
    let readme_path = code_dest.join("README.md");
    fs::write(&readme_path, readme).map_err(|error| {
        anyhow::anyhow!("code repository placeholder README could not be written: {error}")
    })?;
    let commit = crate::core::gitops::commit(
        code_dest,
        &ground_commit_message(title),
        &["README.md".to_owned()],
    )
    .map(|_| ());
    commit.map_err(|error| {
        // Restore the found state (unborn, no placeholder) so a retry
        // starts from the same shape.
        let _ = fs::remove_file(&readme_path);
        anyhow::anyhow!("code repository initial commit failed: {error}")
    })
}

fn code_repository_proof(code_dest: &Path, title: &str) -> anyhow::Result<CodeRepositoryProof> {
    let count_raw = git::output(Some(code_dest), &["rev-list", "--count", "HEAD"])
        .map_err(|error| {
            anyhow::anyhow!(
                "{} could not be inspected by Git: {error}",
                code_dest.display()
            )
        })?
        .stdout;
    let count = std::str::from_utf8(&count_raw)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(u32::MAX);
    match count {
        0 => {
            let porcelain = git::output(Some(code_dest), &["status", "--porcelain"])
                .map_err(|error| {
                    anyhow::anyhow!(
                        "{} could not be inspected by Git: {error}",
                        code_dest.display()
                    )
                })?
                .stdout;
            let loose: Vec<String> = std::str::from_utf8(&porcelain)
                .unwrap_or_default()
                .lines()
                .map(|line| line.trim().to_owned())
                .filter(|line| !line.is_empty())
                .collect();
            let only_leftover_readme = loose == vec!["?? README.md".to_owned()];
            if loose.is_empty() || only_leftover_readme {
                return Ok(CodeRepositoryProof::Unborn);
            }
            refuse_foreign_code_repository(code_dest)
        }
        1 => {
            let subject = git::output(Some(code_dest), &["log", "-1", "--pretty=%s"])
                .map_err(|error| {
                    anyhow::anyhow!(
                        "{} could not be inspected by Git: {error}",
                        code_dest.display()
                    )
                })?
                .stdout;
            let tracked = git::output(Some(code_dest), &["ls-files"])
                .map_err(|error| {
                    anyhow::anyhow!(
                        "{} could not be inspected by Git: {error}",
                        code_dest.display()
                    )
                })?
                .stdout;
            let subject_is_ours = std::str::from_utf8(&subject)
                .is_ok_and(|text| text.trim() == ground_commit_message(title));
            let tracked_is_readme =
                std::str::from_utf8(&tracked).is_ok_and(|text| text.lines().eq(["README.md"]));
            if subject_is_ours && tracked_is_readme && git::status_paths(code_dest)?.is_empty() {
                return Ok(CodeRepositoryProof::Grounded);
            }
            refuse_foreign_code_repository(code_dest)
        }
        _ => refuse_foreign_code_repository(code_dest),
    }
}

fn refuse_foreign_code_repository(code_dest: &Path) -> anyhow::Result<CodeRepositoryProof> {
    anyhow::bail!(
        "{} already exists as a Git repository Kool.ad/e did not create for this project; move it aside, or use a different project name",
        code_dest.display()
    )
}

fn prepare_planning_repository(
    spec: &SiblingPairSpec,
    parent: &Path,
    plan_dest: &Path,
) -> anyhow::Result<()> {
    if is_git_repository_root(plan_dest) {
        WorkspaceManifest::read(plan_dest).map_err(|error| {
            anyhow::anyhow!(
                "{} is a Git repository but its planning manifest is unreadable: {error}",
                plan_dest.display()
            )
        })?;
        anyhow::ensure!(
            git::status_paths(plan_dest)?.is_empty(),
            "{} must be clean before it can be reused as the planning repository; commit or discard the pending planning changes first",
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

/// The pure repository-segment rule behind `validate_repo_name`, exposed so
/// UI layers can refuse unusable names at the input boundary instead of
/// carrying them through a multi-second create flow into this check.
pub(crate) fn repository_name_allowed(name: &str) -> bool {
    !name.starts_with('.')
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
