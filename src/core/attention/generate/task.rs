use std::path::{Component, Path};

pub(super) fn content(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    ticket: &str,
) -> String {
    let relative = Path::new(ticket);
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return "Task story unavailable; rely on the saved report only.".into();
    }
    let stored = planning_store
        .layout()
        .canonical_path(ticket)
        .filter(|path| path.starts_with(planning_store.layout().tasks_root()))
        .and_then(|path| planning_store.read_planning_path(&path).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok());
    stored.map_or_else(
        || "Task story unavailable; rely on the saved report only.".into(),
        |markdown| {
            crate::core::context_build::clip(
                &crate::artifacts::task_docs::visible_content(&markdown),
                18000,
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};

    #[test]
    fn managed_task_content_comes_from_the_selected_store() {
        let root = std::env::temp_dir().join(format!(
            "koolade-managed-attention-task-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store_root = root.join("planning-store");
        let repo = root.join("code");
        let worktree = root.join("task-worktree");
        let ticket = "planning/tasks/demo/task.md";
        std::fs::create_dir_all(&store_root).unwrap();
        std::fs::create_dir_all(repo.join("planning/tasks/demo")).unwrap();
        std::fs::create_dir_all(worktree.join("planning/tasks/demo")).unwrap();
        std::fs::write(
            repo.join(ticket),
            "# Stale code checkout story\n\nStale details.\n",
        )
        .unwrap();
        std::fs::write(
            worktree.join(ticket),
            "# Stale task worktree story\n\nStale details.\n",
        )
        .unwrap();
        let store = PlanningStore::new(uuid::Uuid::new_v4(), &store_root, StoreMode::ManagedShared);
        store
            .atomic_write(
                ticket,
                b"# Current planning story\n\nCurrent authoritative details.\n",
            )
            .unwrap();

        let text = content(&store, ticket);
        assert!(text.contains("Current authoritative details."));
        assert!(!text.contains("Stale"));
        let _ = std::fs::remove_dir_all(root);
    }
}
