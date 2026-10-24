use super::*;

pub(super) fn import_local_source(
    cache: &RepositoryCache,
    repo: &Path,
    source_ref: &str,
    local: &str,
    runner: &Runner,
) -> anyhow::Result<String> {
    let imported = cache.import_local_branch(repo, source_ref, local, runner)?;
    anyhow::ensure!(
        imported == local,
        "Source branch moved while its starting commit was being saved; retry to select the new commit"
    );
    Ok(local.to_owned())
}
