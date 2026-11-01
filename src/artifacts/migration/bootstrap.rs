use super::product;
use std::path::Path;

/// Bootstrap product modules for callers that create state directly instead
/// of entering through the connection pipeline. Normal connection uses
/// `run`, which plans, versions, and checkpoints the same files with every
/// other project artifact.
#[cfg(test)]
pub(crate) fn bootstrap_product(repo: &Path, title: &str) -> anyhow::Result<()> {
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::nil(),
        repo,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    bootstrap_product_with_store(repo, &store, title)?;
    Ok(())
}

pub(crate) fn bootstrap_product_with_store(
    repo: &Path,
    store: &crate::artifacts::planning_store::PlanningStore,
    title: &str,
) -> anyhow::Result<Vec<String>> {
    let expected_revision = store.revision()?;
    bootstrap_product_with_store_expected(repo, store, title, &expected_revision)
        .map(|(paths, _)| paths)
}

pub(crate) fn bootstrap_product_with_store_expected(
    repo: &Path,
    store: &crate::artifacts::planning_store::PlanningStore,
    title: &str,
    expected_revision: &str,
) -> anyhow::Result<(Vec<String>, String)> {
    let generated = product::bootstrap_files_with_store(repo, store, Some(title))?;
    commit_generated(store, generated, expected_revision)
}

/// Test-support bootstrap that lets the generator derive the product title
/// from the seed specification instead of taking an explicit title.
#[cfg(test)]
pub(crate) fn bootstrap_product_from_seed(
    repo: &Path,
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<Vec<String>> {
    let expected_revision = store.revision()?;
    let generated = product::bootstrap_files_with_store(repo, store, None)?;
    commit_generated(store, generated, &expected_revision).map(|(paths, _)| paths)
}

fn commit_generated(
    store: &crate::artifacts::planning_store::PlanningStore,
    generated: Vec<product::Generated>,
    expected_revision: &str,
) -> anyhow::Result<(Vec<String>, String)> {
    let mut changes = Vec::with_capacity(generated.len());
    for file in generated {
        changes.push((file.target, file.bytes));
    }
    let (relative_paths, revision) =
        store.transaction_with_revision(&changes, Some(expected_revision))?;
    Ok((
        relative_paths
            .iter()
            .map(|path| store.git_path(path))
            .collect(),
        revision,
    ))
}
