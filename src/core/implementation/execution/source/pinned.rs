use super::*;

pub(super) fn resolve_pinned_source(
    cache: &RepositoryCache,
    repo: &Path,
    source_sha: &str,
    runner: &Runner,
) -> anyhow::Result<(String, String)> {
    cache.import_local_commit(repo, source_sha, runner)?;
    Ok((source_sha.to_owned(), source_sha.to_owned()))
}
