//! Stage module, manifest, and index updates as one product-document batch.
use crate::artifacts::planning_store::PlanningRoot;

pub(super) fn changes<R: PlanningRoot + ?Sized>(
    repo: &R,
    updates: &[(String, String)],
) -> anyhow::Result<Vec<(String, String)>> {
    anyhow::ensure!(
        !updates.iter().any(|(id, _)| id == "product:index"),
        "The product index is application-maintained"
    );
    let layout = repo.planning_layout();
    let mut changes = Vec::new();
    for (id, content) in updates {
        let path = crate::artifacts::product_docs::document_path_for_update(repo, id, content)?;
        changes.push((
            path.strip_prefix(layout.root())?
                .to_string_lossy()
                .into_owned(),
            content.clone(),
        ));
    }

    let has_product_update = updates.iter().any(|(id, _)| id.starts_with("product:"));
    let has_feature_update = updates.iter().any(|(id, _)| id.starts_with("feature:"));
    if has_product_update {
        let manifest = crate::artifacts::product_docs::updated_manifest(repo, updates)?;
        changes.push((
            crate::artifacts::planning_store::paths::PRODUCT_MANIFEST.into(),
            serde_json::to_string_pretty(&manifest)?,
        ));
    }
    if has_product_update || has_feature_update {
        changes.push((
            crate::artifacts::planning_store::paths::PRODUCT_INDEX.into(),
            crate::artifacts::product_docs::refreshed_index(repo, updates)?,
        ));
    }
    Ok(changes)
}
