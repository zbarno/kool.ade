use super::workflow::safe_directory;
use crate::artifacts::planning_store::PlanningRoot;
use crate::core::workflow::TaskStory;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct ProgressBatch {
    pub(super) run: String,
    pub(super) total: usize,
    pub(super) brief: crate::core::workflow::InterviewBrief,
    pub(super) specification: String,
    pub(super) stories: Vec<TaskStory>,
    #[serde(default)]
    pub(super) task_routing: crate::core::workflow::TaskRoutingSnapshot,
    #[serde(default)]
    pub(super) feature_id: Option<String>,
    #[serde(default)]
    pub(super) identity: Option<crate::domain::ArtifactIdentity>,
}

pub(super) fn progress_batches<R: PlanningRoot + ?Sized>(repo: &R) -> Vec<(String, ProgressBatch)> {
    let mut batches = Vec::new();
    let task_dir = crate::artifacts::koolade::task_dir(repo);
    let layout = repo.planning_layout();
    if let Some(task_root) = layout.canonical_path(&task_dir)
        && let Ok(entries) = std::fs::read_dir(task_root)
    {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let directory = format!("{task_dir}/{}", entry.file_name().to_string_lossy());
            if safe_directory(repo, &directory).is_err() {
                continue;
            }
            if let Ok(bytes) = repo.read_planning_path(&entry.path().join(".koolade-progress.json"))
                && let Ok(text) = String::from_utf8(bytes)
                && let Ok(batch) = serde_json::from_str(&text)
            {
                batches.push((directory, batch));
            }
        }
    }
    batches.sort_by_key(|(directory, _)| {
        repo.planning_layout()
            .canonical_path(directory)
            .and_then(|path| std::fs::metadata(path.join("README.md")).ok())
            .and_then(|m| m.modified().ok())
    });
    batches
}
