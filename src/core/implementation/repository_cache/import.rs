use super::{RepositoryCache, Runner, lock::acquire};
use std::path::Path;

impl RepositoryCache {
    pub(in crate::core::implementation) fn import_local_commit(
        &self,
        source: &Path,
        expected: &str,
        runner: &Runner,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(expected.len(), 40 | 64)
                && expected.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid source commit"
        );
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
            &["cat-file", "-e", &format!("{expected}^{{commit}}")],
        )?;
        Ok(())
    }
}
