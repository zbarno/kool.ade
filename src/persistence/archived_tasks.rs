//! Per-operator archive state for completed Kanban tasks.

use std::collections::BTreeSet;

const FILE: &str = "archived-tasks.json";

pub fn load(slug: &str) -> BTreeSet<String> {
    std::fs::read(super::project_dir(slug).join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(slug: &str, tasks: &BTreeSet<String>) -> Result<(), String> {
    let directory = super::project_dir(slug);
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let target = directory.join(FILE);
    let temporary = directory.join(format!(".{FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(tasks).map_err(|error| error.to_string())?;
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(temporary, target).map_err(|error| error.to_string())
}
