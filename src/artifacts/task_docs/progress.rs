use super::workflow::safe_directory;
use crate::core::workflow::TaskStory;
use std::path::Path;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct ProgressBatch {
    pub(super) run: String,
    pub(super) total: usize,
    pub(super) brief: crate::core::workflow::InterviewBrief,
    pub(super) specification: String,
    pub(super) stories: Vec<TaskStory>,
    #[serde(default)]
    pub(super) feature_id: Option<String>,
    #[serde(default)]
    pub(super) identity: Option<crate::domain::ArtifactIdentity>,
}

pub(super) fn progress_batches(repo: &Path) -> Vec<(String, ProgressBatch)> {
    let mut batches = Vec::new();
    let task_dir = crate::artifacts::koolade::task_dir(repo);
    if let Ok(entries) = std::fs::read_dir(repo.join(&task_dir)) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let directory = format!("{task_dir}/{}", entry.file_name().to_string_lossy());
            if safe_directory(repo, &directory).is_err() {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(entry.path().join(".koolade-progress.json"))
                && let Ok(batch) = serde_json::from_str(&text)
            {
                batches.push((directory, batch));
            }
        }
    }
    batches.sort_by_key(|(directory, _)| {
        std::fs::metadata(repo.join(directory).join("README.md"))
            .and_then(|m| m.modified())
            .ok()
    });
    batches
}

pub(super) fn replace_progress_file(path: &Path, text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()),
        "Refusing linked task file"
    );
    crate::artifacts::atomic_write(path, text)
}
