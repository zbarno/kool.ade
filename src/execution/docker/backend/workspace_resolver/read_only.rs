use std::path::{Path, PathBuf};

use super::super::TaskWorkspaceResolver;
use crate::execution::{
    contract::{WorkspaceRef, is_valid_sha},
    docker::sandbox::ValidatedWorkspace,
    errors::ExecutionError,
};

/// Resolves an app-selected checkout whose source commit is pinned by the
/// request. The Docker policy mounts this workspace read-only.
pub struct ReadOnlyProjectWorkspaceResolver {
    workspace: WorkspaceRef,
    root: PathBuf,
    verified: ValidatedWorkspace,
    staging: PathBuf,
}

impl ReadOnlyProjectWorkspaceResolver {
    pub fn new(
        workspace: WorkspaceRef,
        repository_root: &Path,
        staging_root: &Path,
    ) -> Result<Self, ExecutionError> {
        workspace.validate().map_err(|_| denied())?;
        if std::fs::symlink_metadata(repository_root)
            .map_err(|_| denied())?
            .file_type()
            .is_symlink()
        {
            return Err(denied());
        }
        let root = repository_root.canonicalize().map_err(|_| denied())?;
        let verified = ValidatedWorkspace::inspect(&root)?;
        if !git_head(&root)?.eq_ignore_ascii_case(&workspace.source_sha) {
            return Err(denied());
        }
        let staging = staging_root.canonicalize().map_err(|_| denied())?;
        if !staging.is_dir() {
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

impl TaskWorkspaceResolver for ReadOnlyProjectWorkspaceResolver {
    fn resolve_owned_clone(&self, workspace: &WorkspaceRef) -> Result<PathBuf, ExecutionError> {
        if workspace != &self.workspace {
            return Err(denied());
        }
        self.verified.revalidate()?;
        let current = self.root.canonicalize().map_err(|_| denied())?;
        if current != self.root || !git_head(&current)?.eq_ignore_ascii_case(&workspace.source_sha)
        {
            return Err(denied());
        }
        Ok(current)
    }

    fn staging_root(&self) -> PathBuf {
        self.staging.clone()
    }
}

fn git_head(root: &Path) -> Result<String, ExecutionError> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            root.to_string_lossy().as_ref(),
            "rev-parse",
            "--verify",
            "HEAD",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .map_err(|_| denied())?;
    if !output.status.success() {
        return Err(denied());
    }
    let sha = String::from_utf8(output.stdout).map_err(|_| denied())?;
    let sha = sha.trim();
    if !is_valid_sha(sha) {
        return Err(denied());
    }
    Ok(sha.to_owned())
}

fn denied() -> ExecutionError {
    super::denied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_the_selected_repository_revision() {
        let fixture = fixture();
        let workspace = workspace(&fixture.repo);
        let resolver = ReadOnlyProjectWorkspaceResolver::new(
            workspace.clone(),
            &fixture.repo,
            &fixture.staging,
        )
        .unwrap();
        assert_eq!(
            resolver.resolve_owned_clone(&workspace).unwrap(),
            fixture.repo.canonicalize().unwrap()
        );
        let mut other = workspace;
        other.task_id = "00000000-0000-4000-8000-000000000099".into();
        assert!(resolver.resolve_owned_clone(&other).is_err());
    }

    #[test]
    fn rejects_a_source_revision_mismatch() {
        let fixture = fixture();
        let mut workspace = workspace(&fixture.repo);
        workspace.source_sha = "0".repeat(40);
        assert!(
            ReadOnlyProjectWorkspaceResolver::new(workspace, &fixture.repo, &fixture.staging,)
                .is_err()
        );
    }

    struct Fixture {
        root: PathBuf,
        repo: PathBuf,
        staging: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "koolade-read-only-resolver-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let repo = root.join("repo");
        let staging = root.join("staging");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(repo.join("README.md"), "fixture\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(&repo, &["commit", "-q", "-m", "fixture"]);
        Fixture {
            root,
            repo,
            staging,
        }
    }

    fn workspace(root: &Path) -> WorkspaceRef {
        WorkspaceRef {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            source_sha: git_head(root).unwrap(),
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }
}
