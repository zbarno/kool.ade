use crate::core::implementation::Runner;
use std::path::Path;

const INDEX_REF_ROOT: &str = "refs/koolade/legacy-migration-index";
const MIGRATION_NAME: &str = "Kool.ad/e legacy migration";
const MIGRATION_EMAIL: &str = "koolade-migration@users.noreply.github.com";

pub(super) fn capture(source: &Path, head: &str, runner: &Runner) -> anyhow::Result<String> {
    let tree = runner.git(source, &["write-tree"])?;
    let commit = runner.git(
        source,
        &[
            "-c",
            &format!("user.name={MIGRATION_NAME}"),
            "-c",
            &format!("user.email={MIGRATION_EMAIL}"),
            "commit-tree",
            &tree,
            "-p",
            head,
            "-m",
            "Preserve staged legacy task index",
        ],
    )?;
    let reference = reference(&commit);
    runner.git(source, &["update-ref", &reference, &commit])?;
    Ok(commit)
}

pub(super) fn reference(commit: &str) -> String {
    format!("{INDEX_REF_ROOT}/{commit}")
}

pub(super) fn matches_source(source: &Path, commit: &str, runner: &Runner) -> anyhow::Result<bool> {
    Ok(runner.git(source, &["write-tree"])?
        == runner.git(source, &["rev-parse", &format!("{commit}^{{tree}}")])?)
}

pub(super) fn restore(
    destination: &Path,
    cache: &Path,
    commit: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    let cache_path = cache
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 repository cache path"))?;
    let reference = reference(commit);
    let refspec = format!("{reference}:{reference}");
    runner.git(
        destination,
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            cache_path,
            &refspec,
        ],
    )?;
    anyhow::ensure!(
        runner.git(destination, &["rev-parse", "--verify", &reference])? == commit,
        "Staged merge snapshot differs from its migration record"
    );
    let tree = format!("{commit}^{{tree}}");
    runner.git(destination, &["read-tree", "--reset", "-u", &tree])?;
    Ok(())
}
