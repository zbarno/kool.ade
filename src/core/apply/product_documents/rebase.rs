//! Rebase private proposals only when their published document families are unchanged.
use super::*;
use crate::core::{published_specs::PublishedSpecReader, state::PlannerState};
use std::collections::BTreeMap;

pub(super) fn recover_current(state: &PlannerState, uid: &str) -> anyhow::Result<bool> {
    let store = &state.planning_store;
    let Some(draft) = load_current(store)? else {
        return Ok(false);
    };
    if draft.base_published_spec_pin == store.published_spec_pin {
        return Ok(false);
    }
    let contains = draft.changes.iter().any(|change| {
        change.document_id.starts_with("feature:")
            && crate::domain::ArtifactIdentity::from_markdown(&change.content)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == uid)
    });
    if !contains {
        return Ok(false);
    }
    let base = draft
        .base_published_spec_pin
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("The private draft has no verifiable published base"))?;
    let current = store
        .published_spec_pin
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("The current published revision is unavailable"))?;
    anyhow::ensure!(
        base.repository_id == current.repository_id && base.reference == current.reference,
        "Private draft reconciliation requires the original repository and published ref"
    );
    let repository = store.published_repository_root.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Private draft reconciliation requires its code repository")
    })?;
    let reader = PublishedSpecReader::for_primary_repository(repository)?;
    let before = reader.read_documentation(base)?;
    let after = reader.read_documentation(current)?;
    for doc in &after {
        if !doc.path.starts_with("docs/koolade/features/") {
            continue;
        }
        if let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(&doc.content)? {
            let touches = draft
                .changes
                .iter()
                .any(|change| change.document_id == format!("feature:{}", identity.display_id));
            if touches {
                let metadata = crate::domain::ChangeMetadata::require_markdown(&doc.content)?;
                anyhow::ensure!(
                    !metadata.status.is_terminal(),
                    "Native reconciliation will not reopen a published terminal feature"
                );
            }
        }
    }
    reader.ensure_descendant(base, current)?;
    let product_changed = draft
        .changes
        .iter()
        .any(|change| change.document_id.starts_with("product:"));
    let documents = |docs: Vec<crate::core::published_specs::PublishedDocument>| {
        docs.into_iter()
            .filter(|doc| {
                doc.path.starts_with("docs/koolade/features/")
                    || product_changed && doc.path.starts_with("docs/koolade/product/")
            })
            .map(|doc| (doc.path, doc.content_sha256))
            .collect::<BTreeMap<_, _>>()
    };
    anyhow::ensure!(
        documents(before) == documents(after),
        "Published specification documents changed; the private draft requires explicit reconciliation"
    );
    let cancelled = crate::persistence::cancelled_work::load(store)?;
    let mut identities = std::collections::BTreeSet::new();
    for change in &draft.changes {
        if !change.document_id.starts_with("feature:") {
            continue;
        }
        let metadata = crate::domain::ChangeMetadata::require_markdown(&change.content)?;
        let identity = crate::domain::ArtifactIdentity::from_markdown(&change.content)?
            .ok_or_else(|| {
                anyhow::anyhow!("The private feature draft has no immutable identity")
            })?;
        anyhow::ensure!(
            identity.uid == metadata.uid
                && identity.display_id == metadata.display_id
                && change.document_id == format!("feature:{}", identity.display_id)
                && identities.insert(identity.uid),
            "Private feature identities conflict; explicit reconciliation is required"
        );
        anyhow::ensure!(
            !metadata.status.is_terminal(),
            "Native reconciliation will not reopen a terminal draft"
        );
        anyhow::ensure!(
            !cancelled.contains(&crate::persistence::cancelled_work::planning_id(
                &metadata.uid
            )),
            "Native reconciliation will not restore cancelled work"
        );
        anyhow::ensure!(
            !state
                .active_features
                .iter()
                .any(|(id, _)| Some(id.as_str()) == change.document_id.strip_prefix("feature:")),
            "Published feature display ID is occupied; explicit reconciliation is required"
        );
    }
    let files = snapshot_files(store, draft.changes)?
        .into_iter()
        .map(|(path, text)| (path, text.into_bytes()))
        .collect::<Vec<_>>();
    reader.ensure_local_ref_unchanged(current)?;
    store.transaction(&files, Some(&state.baseline_planning_revision))?;
    Ok(true)
}

#[cfg(test)]
mod tests;
