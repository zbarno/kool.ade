use std::path::{Path, PathBuf};

use super::TaskWorkspaceResolver;
use crate::execution::{
    contract::WorkspaceRef,
    docker::sandbox::ValidatedWorkspace,
    errors::{ExecutionError, ExecutionErrorKind},
};

mod read_only;
pub use read_only::ReadOnlyProjectWorkspaceResolver;

/// Resolves exactly one workspace supplied by trusted application task state.
/// Request data can select the identity, but can never choose a filesystem path.
pub struct OwnedTaskWorkspaceResolver {
    workspace: WorkspaceRef,
    root: PathBuf,
    verified: ValidatedWorkspace,
    staging: PathBuf,
}

impl OwnedTaskWorkspaceResolver {
    pub fn for_current_user(
        workspace: WorkspaceRef,
        task_clone: &Path,
    ) -> Result<Self, ExecutionError> {
        let projects = crate::persistence::state_root().join("projects");
        Self::new(&projects, workspace, task_clone)
    }

    pub fn new(
        projects_root: &Path,
        workspace: WorkspaceRef,
        task_clone: &Path,
    ) -> Result<Self, ExecutionError> {
        workspace.validate().map_err(|_| denied())?;
        let projects_input_meta = std::fs::symlink_metadata(projects_root).map_err(|_| denied())?;
        if !projects_input_meta.is_dir() || projects_input_meta.file_type().is_symlink() {
            return Err(denied());
        }
        let projects = projects_root.canonicalize().map_err(|_| denied())?;
        if std::fs::symlink_metadata(task_clone)
            .map_err(|_| denied())?
            .file_type()
            .is_symlink()
        {
            return Err(denied());
        }
        let root = task_clone.canonicalize().map_err(|_| denied())?;
        let relative = root.strip_prefix(&projects).map_err(|_| denied())?;
        let components = relative
            .components()
            .map(|part| part.as_os_str().to_str().ok_or_else(denied))
            .collect::<Result<Vec<_>, _>>()?;
        if components.len() != 4
            || components[1] != "task-repositories"
            || !safe_component(components[0])
            || !safe_component(components[2])
            || !safe_component(components[3])
        {
            return Err(denied());
        }
        let root_meta = std::fs::symlink_metadata(&root).map_err(|_| denied())?;
        let git_meta = std::fs::symlink_metadata(root.join(".git")).map_err(|_| denied())?;
        if !root_meta.is_dir()
            || root_meta.file_type().is_symlink()
            || !git_meta.is_dir()
            || git_meta.file_type().is_symlink()
        {
            return Err(denied());
        }
        // Retain the directory and `.git` object identities captured at allocation
        // time. Cancellation and recovery must not sync a container back into a
        // different clone that later occupies the same path.
        let verified = ValidatedWorkspace::inspect(&root)?;
        let state_root = projects.parent().ok_or_else(denied)?;
        let staging = state_root.join("execution-staging");
        if std::fs::symlink_metadata(&staging)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(denied());
        }
        std::fs::create_dir_all(&staging).map_err(|_| denied())?;
        let staging = staging.canonicalize().map_err(|_| denied())?;
        if staging.parent() != Some(state_root) {
            return Err(denied());
        }
        Ok(Self {
            workspace,
            root,
            verified,
            staging,
        })
    }
}

impl TaskWorkspaceResolver for OwnedTaskWorkspaceResolver {
    fn resolve_owned_clone(&self, workspace: &WorkspaceRef) -> Result<PathBuf, ExecutionError> {
        if workspace != &self.workspace {
            return Err(denied());
        }
        self.verified.revalidate()?;
        let current = self.root.canonicalize().map_err(|_| denied())?;
        if current != self.root {
            return Err(denied());
        }
        Ok(current)
    }

    fn staging_root(&self) -> PathBuf {
        self.staging.clone()
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn denied() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::PolicyDenied,
        "Workspace is not a registered Kool.ad/e task clone",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> WorkspaceRef {
        WorkspaceRef {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            source_sha: "a".repeat(40),
        }
    }

    struct Fixture {
        root: PathBuf,
        projects: PathBuf,
        task: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "koolade-workspace-resolver-{}",
            uuid::Uuid::new_v4()
        ));
        let projects = root.join("projects");
        let task = projects
            .join("sample-project-1234567890abcdef")
            .join("task-repositories")
            .join("root")
            .join("ticket-1234567890abcdef");
        std::fs::create_dir_all(task.join(".git")).unwrap();
        Fixture {
            root,
            projects,
            task,
        }
    }

    #[test]
    fn resolves_only_the_pinned_task_clone_below_user_project_storage() {
        let fixture = fixture();
        let resolver =
            OwnedTaskWorkspaceResolver::new(&fixture.projects, workspace(), &fixture.task).unwrap();
        assert_eq!(
            resolver.resolve_owned_clone(&workspace()).unwrap(),
            fixture.task.canonicalize().unwrap()
        );
        let mut other = workspace();
        other.task_id = "00000000-0000-4000-8000-000000000003".into();
        assert_eq!(
            resolver.resolve_owned_clone(&other).unwrap_err().kind,
            ExecutionErrorKind::PolicyDenied
        );
    }

    #[test]
    fn rejects_checkout_paths_outside_registered_task_clone_layout() {
        let fixture = fixture();
        let external = fixture.task.join("../../../../../../outside");
        assert!(
            OwnedTaskWorkspaceResolver::new(&fixture.projects, workspace(), &external).is_err()
        );
        let ordinary_checkout = fixture.projects.join("checkout");
        std::fs::create_dir_all(ordinary_checkout.join(".git")).unwrap();
        assert!(
            OwnedTaskWorkspaceResolver::new(&fixture.projects, workspace(), &ordinary_checkout)
                .is_err()
        );
    }

    #[test]
    fn rejects_a_replacement_clone_at_the_original_task_path() {
        let fixture = fixture();
        let resolver =
            OwnedTaskWorkspaceResolver::new(&fixture.projects, workspace(), &fixture.task).unwrap();
        let displaced = fixture.task.with_extension("preserved");
        std::fs::rename(&fixture.task, &displaced).unwrap();
        std::fs::create_dir_all(fixture.task.join(".git")).unwrap();

        assert_eq!(
            resolver.resolve_owned_clone(&workspace()).unwrap_err().kind,
            ExecutionErrorKind::PolicyDenied
        );
    }
}
