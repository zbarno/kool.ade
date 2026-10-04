use crate::{app::session::Project, domain::OpenItem, error::AppError};
use std::path::Path;

pub(super) fn persist_assigned_resolutions(
    project: &mut Project,
    previously_synthesized: Vec<OpenItem>,
    previous_config: Option<String>,
    config_path: &Path,
) -> Result<bool, AppError> {
    let mut updated = project.state.clone();
    let stakeholders = &updated.config.stakeholders;
    updated.items.extend(
        previously_synthesized
            .into_iter()
            .filter(|item| stakeholders.owner_exists(&item.category)),
    );
    let resolved = crate::core::ownership::resolve_assigned_gaps(
        &mut updated.items,
        &mut updated.resolved_items,
        &updated.config.stakeholders,
    );
    if resolved.is_empty() {
        return Ok(false);
    }

    let previous_open_items = crate::artifacts::items_io::serialize(&project.state.items);
    let previous_resolved_items = serde_json::to_string_pretty(&project.state.resolved_items)
        .map_err(|error| AppError::Other(error.to_string()))?;
    crate::artifacts::items_io::sort_queue(&mut updated.items);
    let changes = vec![
        (
            crate::artifacts::layout::canonical::OPEN_ITEMS.into(),
            crate::artifacts::items_io::serialize(&updated.items),
        ),
        (
            crate::artifacts::layout::canonical::RESOLVED_ITEMS.into(),
            serde_json::to_string_pretty(&updated.resolved_items)
                .map_err(|error| AppError::Other(error.to_string()))?,
        ),
    ];
    if let Err(error) = crate::artifacts::transaction::apply(&project.state.repo_root, &changes) {
        let restored = restore_config(config_path, previous_config.as_deref());
        let resynced = project
            .state
            .resync()
            .map_err(|refresh| refresh.to_string());
        let detail = match (restored, resynced) {
            (Ok(()), Ok(())) => format!("Could not resolve ownership gaps: {error}"),
            (restore, resync) => format!(
                "Could not resolve ownership gaps: {error}; settings rollback: {}; state refresh: {}",
                display_result(restore),
                display_result(resync)
            ),
        };
        return Err(AppError::Other(detail));
    }
    if let Err(error) = project.state.resync() {
        let previous_items = vec![
            (
                crate::artifacts::layout::canonical::OPEN_ITEMS.into(),
                previous_open_items,
            ),
            (
                crate::artifacts::layout::canonical::RESOLVED_ITEMS.into(),
                previous_resolved_items,
            ),
        ];
        let queue_rollback =
            crate::artifacts::transaction::apply(&project.state.repo_root, &previous_items)
                .map_err(|rollback| rollback.to_string());
        let config_rollback = restore_config(config_path, previous_config.as_deref());
        let state_refresh = project
            .state
            .resync()
            .map_err(|refresh| refresh.to_string());
        return Err(AppError::Other(format!(
            "Resolved ownership items could not be loaded after writing: {error}; queue rollback: {}; settings rollback: {}; state refresh: {}",
            display_result(queue_rollback),
            display_result(config_rollback),
            display_result(state_refresh)
        )));
    }
    Ok(true)
}

pub(super) fn rollback_config_update(
    project: &mut Project,
    path: &Path,
    previous: Option<&str>,
    cause: &str,
) -> AppError {
    let restored = restore_config(path, previous);
    let resynced = project.state.resync().map_err(|error| error.to_string());
    AppError::Other(format!(
        "Could not resync the updated workspace settings: {cause}; settings rollback: {}; state refresh: {}",
        display_result(restored),
        display_result(resynced)
    ))
}

fn restore_config(path: &Path, previous: Option<&str>) -> Result<(), String> {
    match previous {
        Some(contents) => {
            crate::artifacts::atomic_write(path, contents).map_err(|error| error.to_string())
        }
        None if path.exists() => {
            std::fs::remove_file(path).map_err(|error| error.to_string())?;
            crate::artifacts::sync_parent_directory(path).map_err(|error| error.to_string())
        }
        None => Ok(()),
    }
}

fn display_result<T, E: std::fmt::Display>(result: Result<T, E>) -> String {
    match result {
        Ok(_) => "succeeded".into(),
        Err(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rollback_restores_previous_file_or_removes_new_file() {
        let root = std::env::temp_dir().join(format!(
            "koolade_settings_rollback_{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("project.md");
        std::fs::write(&config, "new settings").unwrap();

        restore_config(&config, Some("old settings")).unwrap();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "old settings");
        restore_config(&config, None).unwrap();
        assert!(!config.exists());

        std::fs::remove_dir_all(root).unwrap();
    }
}
