use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    artifacts::{
        atomic_write_bytes,
        planning_store::{PlanningStore, StoreMode},
    },
    core::{gitops, project_repos::ProjectManifest, state::PlannerState},
};

use super::super::git;

const GITIGNORE: &str = "# Operator-local secrets and runtime data
.env
.env.*
credentials/
credentials.*
*.pem
*.key
*.p12
implementation/
**/implementation/
logs/
**/logs/
tmp/
**/tmp/
*.log
*.tmp
*.temp
*.swp
";

const COLLECTION_DIRECTORIES: &[&str] = &[
    "config",
    "planning",
    "planning/changes",
    "planning/tasks",
    "planning/decisions",
    "planning/imports",
    "planning/items",
    "state",
    "state/work",
    "state/workflow",
    "state/items",
    "state/tasks",
];

pub(in crate::core::managed_workspace) fn populate_and_commit(
    planning_repo: &Path,
    workspace_id: uuid::Uuid,
    title: &str,
) -> anyhow::Result<()> {
    let root = planning_repo.canonicalize()?;
    anyhow::ensure!(
        git::repository_root(&root)? == root,
        "planning staging directory is not a Git repository root"
    );
    ensure_collection_directories(&root)?;
    let mut manifest = super::super::manifest::WorkspaceManifest::new(workspace_id);
    manifest.title = Some(title.to_owned());
    ensure_exact_file(
        &root.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    ensure_gitignore(&root.join(".gitignore"))?;
    let registry_path = root.join("config/repositories.json");
    let empty_registry = ProjectManifest {
        repositories: Vec::new(),
    };
    ensure_exact_file(&registry_path, &serde_json::to_vec_pretty(&empty_registry)?)?;
    let store = PlanningStore::new(workspace_id, &root, StoreMode::ManagedLocal);
    store.recover()?;
    crate::artifacts::migration::bootstrap_product_with_store(&root, &store, title)?;
    let mut state = PlannerState::load_with_store(&root, &store)?;
    state.title = title.to_owned();
    let _created = state.bootstrap_missing()?;
    let registry = ProjectManifest::load(&store)?;
    anyhow::ensure!(
        registry.repositories.is_empty(),
        "fresh planning workspace unexpectedly contains a code repository"
    );

    verify_initial_tree(&root)?;
    let status = git::status_paths(&root)?;
    anyhow::ensure!(
        status.iter().all(|path| tracked_artifact_path(path)),
        "planning staging repository contains a non-planning artifact"
    );
    if !git::has_commit(&root)? {
        let paths = status.into_iter().collect::<BTreeSet<_>>();
        anyhow::ensure!(
            !paths.is_empty(),
            "fresh planning repository has nothing to commit"
        );
        gitops::commit(
            &root,
            "Kool.ad/e: initialize planning workspace",
            &paths.into_iter().collect::<Vec<_>>(),
        )
        .map_err(|error| anyhow::anyhow!("initial planning commit failed: {error}"))?;
    } else {
        anyhow::ensure!(
            git::status_paths(&root)?.is_empty(),
            "staging repository changed after its initial commit; recovery stopped safely"
        );
    }
    anyhow::ensure!(
        git::status_paths(&root)?.is_empty(),
        "planning repository is not clean after its initial commit"
    );
    let branch = git::run(Some(&root), &["symbolic-ref", "--short", "HEAD"])?;
    anyhow::ensure!(
        String::from_utf8(branch.stdout)?.trim() == "main",
        "fresh planning repository must use the main branch"
    );
    super::super::manifest::WorkspaceManifest::read(&root)?;
    Ok(())
}

fn ensure_collection_directories(root: &Path) -> anyhow::Result<()> {
    for relative in COLLECTION_DIRECTORIES {
        fs::create_dir_all(root.join(relative))?;
    }
    Ok(())
}

fn ensure_gitignore(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        let existing = fs::read_to_string(path)?;
        for required in [
            ".env.*",
            "credentials/",
            "implementation/",
            "*.log",
            "*.tmp",
        ] {
            anyhow::ensure!(
                existing.lines().any(|line| line.trim() == required),
                "planning .gitignore is missing required rule {required}"
            );
        }
        Ok(())
    } else {
        atomic_write_bytes(path, GITIGNORE.as_bytes())?;
        Ok(())
    }
}

fn ensure_exact_file(path: &Path, expected: &[u8]) -> anyhow::Result<()> {
    if path.exists() {
        let existing = fs::read(path)?;
        anyhow::ensure!(
            existing == expected,
            "staging artifact {} differs from the expected fresh workspace value",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        Ok(())
    } else {
        atomic_write_bytes(path, expected)?;
        Ok(())
    }
}

fn verify_initial_tree(root: &Path) -> anyhow::Result<()> {
    let mut directories = BTreeSet::new();
    for relative in COLLECTION_DIRECTORIES {
        directories.insert((*relative).to_owned());
    }
    let mut files = Vec::new();
    collect_tree(root, root, &mut files, &mut directories)?;
    for directory in directories {
        anyhow::ensure!(
            allowed_directory(&directory),
            "unexpected directory in fresh planning repository: {directory}"
        );
    }
    for file in files {
        let relative = file
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        anyhow::ensure!(
            tracked_artifact_path(&relative),
            "unexpected file in fresh planning repository: {relative}"
        );
    }
    for required in [
        "manifest.json",
        ".gitignore",
        "config/project.md",
        "config/repositories.json",
        "planning/product/index.md",
        "planning/product/manifest.json",
    ] {
        anyhow::ensure!(
            root.join(required).is_file(),
            "fresh planning repository is missing {required}"
        );
    }
    for forbidden in [
        "planning/open-items.md",
        "planning/resolved-items.json",
        "state/workflow.json",
        "state/work.json",
    ] {
        anyhow::ensure!(
            !root.join(forbidden).exists(),
            "fresh planning repository created legacy authority {forbidden}"
        );
    }
    Ok(())
}

fn collect_tree(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
    directories: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative == ".git" || relative.starts_with(".git/") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "symbolic links are not permitted in a fresh planning repository"
        );
        if metadata.is_dir() {
            directories.insert(relative);
            collect_tree(root, &path, files, directories)?;
        } else if metadata.is_file() {
            files.push(path);
        } else {
            anyhow::bail!("unsupported filesystem entry in planning repository");
        }
    }
    Ok(())
}

fn tracked_artifact_path(path: &str) -> bool {
    matches!(
        path,
        "manifest.json" | ".gitignore" | "config/project.md" | "config/repositories.json"
    ) || path.starts_with("planning/product/")
}

fn allowed_directory(path: &str) -> bool {
    COLLECTION_DIRECTORIES.contains(&path)
        || path == "planning/product"
        || path.starts_with("planning/product/")
}

pub(in crate::core::managed_workspace) fn sync_tree(root: &Path) -> io::Result<()> {
    sync_directory_tree(root)
}

fn sync_directory_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other("symbolic link in planning repository"));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            sync_directory_tree(&entry?.path())?;
        }
        return super::super::manager::sync_directory(path);
    }
    fs::File::open(path)?.sync_all()
}
