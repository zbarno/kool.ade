use std::path::{Component, Path};

pub(super) fn content(worktree: &Path, repo: &Path, ticket: &str) -> String {
    let relative = Path::new(ticket);
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return "Task story unavailable; rely on the saved report only.".into();
    }
    for root in [worktree, repo] {
        let Ok(canonical_root) = root.canonicalize() else {
            continue;
        };
        let Ok(path) = root.join(relative).canonicalize() else {
            continue;
        };
        if !path.starts_with(&canonical_root) {
            continue;
        }
        let Ok(markdown) = std::fs::read_to_string(path) else {
            continue;
        };
        let visible = crate::artifacts::task_docs::visible_content(&markdown);
        return crate::core::context_build::clip(&visible, 18000);
    }
    "Task story unavailable; rely on the saved report only.".into()
}
