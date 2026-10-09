use super::{RepositoryCache, Runner, lock::acquire};
use std::path::Path;

impl RepositoryCache {
    pub(in crate::core::implementation) fn import_ref(
        &self,
        source: &Path,
        source_ref: &str,
        destination_ref: &str,
        expected: &str,
        runner: &Runner,
    ) -> anyhow::Result<()> {
        runner.git(&self.path, &["check-ref-format", source_ref])?;
        runner.git(&self.path, &["check-ref-format", destination_ref])?;
        let _guard = acquire(&self.path, runner)?;
        if let Ok(existing) = runner.git(&self.path, &["rev-parse", "--verify", destination_ref]) {
            anyhow::ensure!(
                existing == expected,
                "Imported repository reference differs from its migration record"
            );
            return Ok(());
        }
        let source_path = source
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 source repository path"))?;
        let refspec = format!("{source_ref}:{destination_ref}");
        runner.git(
            &self.path,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                source_path,
                &refspec,
            ],
        )?;
        anyhow::ensure!(
            runner.git(&self.path, &["rev-parse", "--verify", destination_ref])? == expected,
            "Imported repository reference differs from its migration record"
        );
        Ok(())
    }
}
