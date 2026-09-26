use crate::core::workflow::Workflow;
use std::path::Path;

/// Refuse links in app-owned paths before creating directories or files.
pub fn safe_directory(repo: &Path, relative: &str) -> anyhow::Result<()> {
    let mut path = repo.to_path_buf();
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

pub fn save_workflow(repo: &Path, workflow: &Workflow) -> anyhow::Result<()> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    safe_directory(repo, crate::artifacts::layout::canonical::STATE)?;
    let target = layout.workflow_state();
    if let Ok(meta) = std::fs::symlink_metadata(&target) {
        anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Workflow must be a regular file"
        );
    }
    crate::artifacts::atomic_write(&target, &serde_json::to_string_pretty(workflow)?)
}

pub fn load_workflow(repo: &Path) -> anyhow::Result<Workflow> {
    match std::fs::read_to_string(
        crate::artifacts::layout::ArtifactLayout::new(repo).workflow_state(),
    ) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Workflow::default()),
        Err(e) => Err(e.into()),
    }
}
