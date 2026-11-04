use crate::artifacts::planning_store::PlanningRoot;
use crate::core::workflow::Workflow;
use std::path::Path;

/// Refuse links in app-owned paths before creating directories or files.
pub fn safe_directory<R: PlanningRoot + ?Sized>(repo: &R, relative: &str) -> anyhow::Result<()> {
    let layout = repo.planning_layout();
    let root = layout.root();
    if !root.exists() {
        std::fs::create_dir_all(root)?;
    }
    let root_metadata = std::fs::symlink_metadata(root)?;
    anyhow::ensure!(
        root_metadata.is_dir() && !root_metadata.file_type().is_symlink(),
        "{} must be a real directory",
        root.display()
    );
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        anyhow::ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "Invalid planning directory"
        );
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "{} must be a real directory",
                path.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&path)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn save_workflow<R: PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &Workflow,
) -> anyhow::Result<()> {
    let store = repo.planning_store();
    let (changes, checks) = super::record_files::changes(repo, workflow)?;
    if changes.is_empty() {
        return Ok(());
    }
    store.transaction_with_record_revisions(&changes, &checks)?;
    Ok(())
}

pub fn load_workflow<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Workflow> {
    let store = repo.planning_store();
    store.with_consistent_read(|| load_workflow_unlocked(repo))
}

pub(crate) fn load_workflow_unlocked<R: PlanningRoot + ?Sized>(
    repo: &R,
) -> anyhow::Result<Workflow> {
    super::record_files::load_unlocked(repo)
}
