mod commit_identity;
mod endpoints;
mod identity;
mod import;
mod lock;
mod operations;
mod saved_state;
mod task_commits;

pub(crate) use commit_identity::{
    GitCommitIdentity, configure_clone, load_or_capture as load_task_git_identity,
    read_for_task as read_task_git_identity, save_for_task as save_task_git_identity,
};

use super::Runner;
use crate::artifacts::planning_store::PlanningRoot;
use std::path::{Path, PathBuf};

use endpoints::{configured_raw_value, configured_value, effective_remote_url};
use identity::{identity_for_repository, sanitize_remote};
use lock::acquire;
use operations::{
    CloneRemotes, CloneRequest, create_independent_clone, ensure_bare_cache, verify_bare_cache,
    verify_independent_repository,
};

pub(super) struct RepositoryCache {
    pub(super) identity: String,
    pub(super) path: PathBuf,
    /// Sanitized manifest URL used for GitHub identity and clone configuration.
    pub(super) origin_url: Option<String>,
    /// Sanitized effective push URL used by the app-owned cache for pushes.
    pub(super) push_url: Option<String>,
    /// Configured push URL used to identify the PR and CI repository.
    pub(super) push_identity_url: Option<String>,
    /// Sanitized effective fetch URL. It is used only for explicit cache fetches.
    pub(super) fetch_url: Option<String>,
}

impl RepositoryCache {
    pub(super) fn open<R: PlanningRoot + ?Sized>(
        repo: &Path,
        planning_root: &R,
        repository_id: &str,
        runner: &Runner,
    ) -> anyhow::Result<Self> {
        Self::open_at(repo, planning_root, repository_id, None, runner)
    }

    pub(super) fn open_at<R: PlanningRoot + ?Sized>(
        repo: &Path,
        planning_root: &R,
        repository_id: &str,
        saved_path: Option<&Path>,
        runner: &Runner,
    ) -> anyhow::Result<Self> {
        let manifest = crate::core::project_repos::ProjectManifest::load(planning_root)?;
        let repository = manifest
            .repositories
            .iter()
            .find(|repository| repository.id == repository_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown task repository {repository_id}"))?;
        let origin_url = if repository.remote.is_empty() {
            None
        } else {
            Some(sanitize_remote(&repository.remote)?)
        };
        let fetch_url = effective_remote_url(repo, false, runner)?;
        let push_url = effective_remote_url(repo, true, runner)?;
        let push_identity_url =
            configured_value(repo, "remote.origin.pushurl", runner)?.or_else(|| origin_url.clone());
        let identity = identity_for_repository(
            &repository.remote,
            fetch_url.as_deref(),
            push_url.as_deref(),
            repo,
        )?;
        let expected_path = cache_path(&identity)?;
        let path = saved_path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| expected_path.clone());
        anyhow::ensure!(
            path == expected_path,
            "Saved repository cache path does not match its origin and fetch/push endpoints"
        );
        let cache = Self {
            identity,
            path,
            origin_url,
            push_url,
            push_identity_url,
            fetch_url,
        };
        cache.ensure(repo, runner)?;
        Ok(cache)
    }

    fn ensure(&self, source: &Path, runner: &Runner) -> anyhow::Result<()> {
        let _guard = acquire(&self.path, runner)?;
        ensure_bare_cache(
            source,
            &self.path,
            self.origin_url.as_deref(),
            self.push_url.as_deref(),
            self.fetch_url.as_deref(),
            runner,
        )
    }

    pub(super) fn refresh_branch(&self, branch: &str, runner: &Runner) -> anyhow::Result<String> {
        let remote = self.fetch_url.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Repository has no fetch remote; select a local source branch")
        })?;
        self.validate_branch(branch, runner)?;
        let _guard = acquire(&self.path, runner)?;
        let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
        runner.git(
            &self.path,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                remote,
                &refspec,
            ],
        )?;
        self.resolve_branch(branch, runner)
    }

    pub(super) fn validate_task_remote(
        &self,
        task_repository: &Path,
        runner: &Runner,
    ) -> anyhow::Result<()> {
        let origin = self
            .origin_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Saved task has no trusted Git origin"))?;
        let actual_origin = configured_raw_value(task_repository, "remote.origin.url", runner)?
            .ok_or_else(|| anyhow::anyhow!("Task repository origin is missing"))?;
        anyhow::ensure!(
            actual_origin == origin,
            "Task repository origin changed; refusing to publish"
        );
        let expected_push = self.push_url.as_deref().unwrap_or(origin);
        let actual_push = configured_raw_value(task_repository, "remote.origin.pushurl", runner)?
            .unwrap_or_else(|| actual_origin.clone());
        anyhow::ensure!(
            actual_push == expected_push,
            "Task repository push destination changed; refusing to publish"
        );
        Ok(())
    }

    pub(super) fn import_local_branch(
        &self,
        source: &Path,
        branch: &str,
        expected: &str,
        runner: &Runner,
    ) -> anyhow::Result<String> {
        self.validate_branch(branch, runner)?;
        let _guard = acquire(&self.path, runner)?;
        if runner
            .git(
                &self.path,
                &["cat-file", "-e", &format!("{expected}^{{commit}}")],
            )
            .is_err()
        {
            let source_path = source
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Non-UTF8 source repository path"))?;
            runner.git(
                &self.path,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    source_path,
                    expected,
                ],
            )?;
        }
        runner.git(
            &self.path,
            &["update-ref", &format!("refs/heads/{branch}"), expected],
        )?;
        self.resolve_branch(branch, runner)
    }

    pub(super) fn resolve_branch(&self, branch: &str, runner: &Runner) -> anyhow::Result<String> {
        runner.git(
            &self.path,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/heads/{branch}^{{commit}}"),
            ],
        )
    }

    pub(super) fn pin_source(&self, commit: &str, runner: &Runner) -> anyhow::Result<String> {
        anyhow::ensure!(
            matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid source commit"
        );
        let branch = format!("koolade-cache/{commit}");
        self.validate_branch(&branch, runner)?;
        let _guard = acquire(&self.path, runner)?;
        runner.git(
            &self.path,
            &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        )?;
        runner.git(
            &self.path,
            &["update-ref", &format!("refs/heads/{branch}"), commit],
        )?;
        Ok(branch)
    }

    pub(super) fn validate_branch(&self, branch: &str, runner: &Runner) -> anyhow::Result<()> {
        runner.git(&self.path, &["check-ref-format", "--branch", branch])?;
        Ok(())
    }

    pub(super) fn create_clone(
        &self,
        source_branch: &str,
        source_commit: &str,
        task_branch: &str,
        destination: &Path,
        commit_identity: &GitCommitIdentity,
        runner: &Runner,
    ) -> anyhow::Result<()> {
        self.validate_branch(source_branch, runner)?;
        self.validate_branch(task_branch, runner)?;
        let _guard = acquire(&self.path, runner)?;
        anyhow::ensure!(
            self.resolve_branch(source_branch, runner)? == source_commit,
            "Pinned source ref no longer resolves to the saved starting commit"
        );
        create_independent_clone(
            &self.path,
            CloneRequest {
                source_branch,
                source_commit,
                task_branch,
                destination,
                commit_identity,
                remotes: CloneRemotes {
                    origin: self.origin_url.as_deref(),
                    push: self.push_url.as_deref(),
                },
            },
            runner,
        )
    }

    pub(super) fn verify_task_repository(path: &Path, runner: &Runner) -> anyhow::Result<()> {
        verify_independent_repository(path, runner)
    }
}

pub(super) fn cache_path(identity: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        identity.len() == 64 && identity.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid repository identity"
    );
    Ok(crate::persistence::state_root()
        .join("repositories")
        .join(format!("{identity}.git")))
}
