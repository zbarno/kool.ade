//! Restore an exact, unambiguous private feature snapshot without changing identity.
use super::*;
use crate::core::state::PlannerState;

pub(crate) fn restore_missing_feature(
    state: &PlannerState,
    uid: &str,
) -> anyhow::Result<PlannerState> {
    let store = &state.planning_store;
    let revision = store.revision()?;
    anyhow::ensure!(
        revision == state.baseline_planning_revision,
        "Planning state changed before native triage; refresh before recovery"
    );
    let files = store.list_files("state/spec_drafts")?;
    anyhow::ensure!(
        files.len() <= 256,
        "Native triage snapshot budget exceeded; inspect the private draft history before repair"
    );
    let mut candidate: Option<LocalSpecDraftChange> = None;
    for file in files {
        let Some(digest) = file
            .name
            .strip_suffix(".json")
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        else {
            continue;
        };
        anyhow::ensure!(
            file.bytes <= 4 * 1024 * 1024,
            "Private draft snapshot exceeds recovery size budget"
        );
        let bytes = store.read(format!("state/spec_drafts/{}", file.name))?;
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == digest,
            "Private draft snapshot digest mismatch"
        );
        let snapshot: StoredSpecDraftV1 = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            snapshot.schema_version == 1,
            "Unsupported private draft snapshot"
        );
        if snapshot.base_published_spec_pin != store.published_spec_pin {
            continue;
        }
        validate_changes(&snapshot.changes)?;
        for change in snapshot.changes {
            if !change.document_id.starts_with("feature:") {
                continue;
            }
            let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(&change.content)?
            else {
                continue;
            };
            if identity.uid != uid {
                continue;
            }
            anyhow::ensure!(
                candidate
                    .as_ref()
                    .is_none_or(|existing| existing == &change),
                "Multiple different snapshots exist for the missing feature UID; explicit reconciliation is required"
            );
            candidate = Some(change);
        }
    }
    let candidate = candidate.ok_or_else(|| anyhow::anyhow!("No verified snapshot of the exact missing feature UID exists at the current published revision"))?;
    let feature_id = candidate.document_id.strip_prefix("feature:").unwrap();
    if let Some(current) =
        crate::artifacts::product_docs::current_document(store, &candidate.document_id)?
    {
        let identity = crate::domain::ArtifactIdentity::from_markdown(&current)?;
        anyhow::ensure!(
            identity.is_some_and(|identity| identity.uid == uid),
            "The current document has a different feature identity; native recovery will not replace it"
        );
        let metadata = crate::domain::ChangeMetadata::require_markdown(&current)?;
        anyhow::ensure!(
            !metadata.status.is_terminal(),
            "The current feature was completed or abandoned; native recovery will not reopen it"
        );
    }
    let cancelled = crate::persistence::cancelled_work::load(store)?;
    anyhow::ensure!(
        !cancelled.contains(&crate::persistence::cancelled_work::planning_id(uid)),
        "The feature was cancelled; native recovery will not restore it"
    );

    anyhow::ensure!(
        !state.active_features.iter().any(|(id, _)| id == feature_id),
        "The missing feature's display ID is occupied; recovery will not overwrite a different identity"
    );
    let metadata = crate::domain::ChangeMetadata::require_markdown(&candidate.content)?;
    anyhow::ensure!(
        !metadata.status.is_terminal(),
        "The feature snapshot is terminal; recovery will not reopen completed or abandoned work"
    );
    let files = changes(store, &[(candidate.document_id, candidate.content)])?;
    let changes = files
        .into_iter()
        .map(|(p, s)| (p, s.into_bytes()))
        .collect::<Vec<_>>();
    // The final fence covers concurrent draft changes, removal and cancellation.
    store.transaction(&changes, Some(&revision))?;
    let refreshed = PlannerState::load_with_store(&state.repo_root, store)?;
    anyhow::ensure!(
        refreshed.active_features.iter().any(|(_, body)| {
            crate::domain::ArtifactIdentity::from_markdown(body)
                .ok()
                .flatten()
                .is_some_and(|i| i.uid == uid)
        }),
        "Restored draft is not active; native repair did not pass verification"
    );
    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Production boundary: verified immutable private snapshots -> fenced draft
    /// restoration -> real pinned Git state loading. A unit validator cannot prove
    /// draft projection or preservation of terminal replacements. Temporary Git
    /// repository and local files only; no Docker/network, low setup cost.
    #[test]
    fn native_repair_restores_exact_uid_and_refuses_terminal_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let code = dir.path().join("code");
        crate::core::state::test_support::seed_published_product_document(&code).unwrap();
        let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
            uuid::Uuid::new_v4(),
            dir.path().join("private"),
            &code,
        );
        let initial = PlannerState::load_with_store(&code, &store).unwrap();
        let store = initial.planning_store;
        let text = "# F1: Recover feature\n\n## Intent\n\nKeep intent.\n\n## Current Behavior\n\nMissing draft.\n\n## Desired Behavior\n\nRestore draft.\n\n## Scope\n\nLocal recovery.\n\n## Affected Product Areas\n\nPlanning.\n\n## Requirements\n\nPreserve identity.\n\n## Decisions and Assumptions\n\nNo new decisions.\n\n## Acceptance Criteria\n\nSame UID survives.\n";
        let original =
            crate::test_support::current_change_fixture(text, crate::domain::ChangeStatus::Draft)
                .unwrap();
        let uid = crate::domain::ArtifactIdentity::from_markdown(&original)
            .unwrap()
            .unwrap()
            .uid;
        persist(
            &store,
            &changes(&store, &[("feature:F1".into(), original)]).unwrap(),
        );
        std::fs::remove_file(store.root.join("state/spec_drafts/current.json")).unwrap();
        let missing = PlannerState::load_with_store(&code, &store).unwrap();
        assert!(missing.active_features.is_empty());
        let repaired = restore_missing_feature(&missing, &uid).unwrap();
        assert!(repaired.active_features.iter().any(|(_, body)| {
            crate::domain::ArtifactIdentity::from_markdown(body)
                .unwrap()
                .unwrap()
                .uid
                == uid
        }));
        let replacement = crate::test_support::current_change_fixture(
            text,
            crate::domain::ChangeStatus::Abandoned,
        )
        .unwrap();
        persist(
            &store,
            &changes(&store, &[("feature:F1".into(), replacement.clone())]).unwrap(),
        );
        let current = PlannerState::load_with_store(&code, &store).unwrap();
        assert!(current.active_features.is_empty());
        let before = store.revision().unwrap();
        assert!(
            restore_missing_feature(&current, &uid)
                .unwrap_err()
                .to_string()
                .contains("different feature identity")
        );
        assert_eq!(store.revision().unwrap(), before);
        assert_eq!(
            crate::artifacts::product_docs::current_document(&store, "feature:F1")
                .unwrap()
                .unwrap(),
            replacement
        );
    }

    fn persist(
        store: &crate::artifacts::planning_store::PlanningStore,
        files: &[(String, String)],
    ) {
        let changes = files
            .iter()
            .map(|(p, s)| (p.clone(), s.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        store
            .transaction(&changes, Some(&store.revision().unwrap()))
            .unwrap();
    }
}
