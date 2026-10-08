use std::path::Path;

pub(super) fn inside_workspace(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
        || crate::persistence::state_root()
            .canonicalize()
            .is_ok_and(|state_root| root.starts_with(&state_root) && path.starts_with(&state_root))
        || root
            .ancestors()
            .find(|ancestor| {
                ancestor
                    .file_name()
                    .is_some_and(|name| name == ".koolade-worktrees")
            })
            .and_then(Path::parent)
            .is_some_and(|workspace| path.starts_with(workspace))
}
