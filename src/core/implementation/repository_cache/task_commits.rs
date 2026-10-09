use super::RepositoryCache;
use crate::core::implementation::Runner;

impl RepositoryCache {
    pub(in crate::core::implementation) fn push_commit(
        &self,
        source: &std::path::Path,
        commit: &str,
        destination_ref: &str,
        runner: &Runner,
    ) -> anyhow::Result<()> {
        validate_commit(commit)?;
        runner.git(&self.path, &["check-ref-format", destination_ref])?;
        let push_url = self
            .push_url
            .as_deref()
            .or(self.origin_url.as_deref())
            .ok_or_else(|| anyhow::anyhow!("Saved task has no trusted push destination"))?;
        Self::verify_task_repository(source, runner)?;
        let _guard = super::lock::acquire(&self.path, runner)?;
        if runner
            .git(
                &self.path,
                &["cat-file", "-e", &format!("{commit}^{{commit}}")],
            )
            .is_err()
        {
            runner.remaining()?;
            let source = source
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Non-UTF8 task repository path"))?;
            runner.git(
                &self.path,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    source,
                    commit,
                ],
            )?;
        }
        anyhow::ensure!(
            runner
                .git(
                    &self.path,
                    &["cat-file", "-e", &format!("{commit}^{{commit}}")]
                )?
                .is_empty(),
            "Candidate commit was not imported into the app-owned cache"
        );
        runner.git(
            &self.path,
            &["push", push_url, &format!("{commit}:{destination_ref}")],
        )?;
        Ok(())
    }

    pub(in crate::core::implementation) fn pin_evidence(
        &self,
        ticket_key: &str,
        commit: &str,
        runner: &Runner,
    ) -> anyhow::Result<()> {
        validate_commit(commit)?;
        let _guard = super::lock::acquire(&self.path, runner)?;
        let reference = format!("refs/koolade-evidence/{ticket_key}/{commit}");
        runner.git(
            &self.path,
            &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        )?;
        runner.git(&self.path, &["update-ref", &reference, commit])?;
        Ok(())
    }

    pub(in crate::core::implementation) fn pin_task_commit(
        &self,
        task_repository: &std::path::Path,
        branch: &str,
        base_commit: &str,
        commit: &str,
        ticket_key: &str,
        runner: &Runner,
    ) -> anyhow::Result<String> {
        validate_commit(commit)?;
        validate_commit(base_commit)?;
        Self::verify_task_repository(task_repository, runner)?;
        anyhow::ensure!(
            runner.git(task_repository, &["symbolic-ref", "--short", "HEAD"])? == branch
                && runner.git(task_repository, &["rev-parse", "HEAD"])? == commit
                && runner
                    .git(
                        task_repository,
                        &["status", "--porcelain", "--untracked-files=no"]
                    )?
                    .is_empty(),
            "Verified task clone has uncommitted tracked changes before integration; preserved for review"
        );
        anyhow::ensure!(
            runner
                .git(
                    task_repository,
                    &["merge-base", "--is-ancestor", base_commit, commit]
                )
                .is_ok(),
            "Verified task commit no longer descends from its pinned starting commit"
        );
        self.validate_branch(branch, runner)?;
        let reference = Self::task_commit_ref(ticket_key, commit);
        let source = task_repository
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 task repository path"))?;
        let refspec = format!("+refs/heads/{branch}:{reference}");
        let _guard = super::lock::acquire(&self.path, runner)?;
        runner.git(
            &self.path,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                source,
                &refspec,
            ],
        )?;
        anyhow::ensure!(
            runner.git(
                &self.path,
                &["rev-parse", &format!("{reference}^{{commit}}")]
            )? == commit,
            "App-owned cache did not retain the verified task commit"
        );
        Ok(reference)
    }

    pub(in crate::core::implementation) fn task_commit_ref(
        ticket_key: &str,
        commit: &str,
    ) -> String {
        format!("refs/koolade-tasks/{ticket_key}/{commit}")
    }
}

fn validate_commit(commit: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid task commit identity"
    );
    Ok(())
}
