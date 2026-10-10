//! Koolade-owned identities and durable feature links for open items.
use crate::artifacts::planning_store::PlanningRoot;
use crate::core::state::PlannerState;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn preserve_feature_updates<R: PlanningRoot + ?Sized>(
    repo: &R,
    updates: &mut [(String, String)],
    statuses: &std::collections::BTreeMap<String, crate::domain::ChangeStatus>,
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut feature_uids = BTreeMap::new();
    for (id, content) in updates {
        let Some(feature_id) = id.strip_prefix("feature:") else {
            continue;
        };
        let path = crate::artifacts::product_docs::document_path_for_update(repo, id, content)?;
        let previous = match repo.read_planning_path(&path) {
            Ok(bytes) => Some(String::from_utf8(bytes)?),
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        };
        let previous_status = previous
            .as_deref()
            .map(crate::domain::ChangeMetadata::require_markdown)
            .transpose()?
            .map(|metadata| metadata.status);
        let clean_content = crate::domain::ChangeMetadata::strip_markers(content);
        *content =
            crate::artifacts::product_docs::identity::preserve_feature_identity_with_previous(
                previous.as_deref(),
                feature_id,
                &clean_content,
            )?;
        let identity = crate::domain::ArtifactIdentity::from_markdown(content)?
            .ok_or_else(|| anyhow::anyhow!("Feature identity was not written"))?;
        let status = statuses
            .get(feature_id)
            .copied()
            .or(previous_status)
            .ok_or_else(|| anyhow::anyhow!("New feature {feature_id} has no typed status"))?;
        if let Some(previous) = previous_status {
            anyhow::ensure!(
                previous.can_transition_to(status),
                "Feature {feature_id} cannot transition from {} to {}",
                previous.wire_name(),
                status.wire_name()
            );
        }
        *content = crate::domain::ChangeMetadata::write_markdown(content, &identity, status)?;
        anyhow::ensure!(
            feature_uids
                .insert(feature_id.to_owned(), identity.uid)
                .is_none(),
            "Feature {feature_id} is updated more than once in a turn"
        );
    }
    Ok(feature_uids)
}

pub(super) fn stabilize_open_item_identities(
    state: &mut PlannerState,
    feature_uids: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let mut seen = BTreeSet::new();
    for item in state
        .items
        .iter_mut()
        .chain(state.resolved_items.iter_mut())
    {
        let uid = item
            .uid
            .get_or_insert_with(|| uuid::Uuid::new_v4().hyphenated().to_string());
        anyhow::ensure!(
            seen.insert(uid.clone()),
            "Duplicate open item identity {uid}"
        );
        let Some(feature_id) = &item.feature_id else {
            continue;
        };
        let feature_uid = if let Some(uid) = feature_uids.get(feature_id) {
            Some(uid.clone())
        } else {
            let Ok(path) = crate::artifacts::product_docs::document_path(
                &state.planning_store,
                &format!("feature:{feature_id}"),
            ) else {
                continue;
            };
            let text = String::from_utf8(state.planning_store.read_planning_path(&path)?)?;
            crate::domain::ArtifactIdentity::from_markdown(&text)?.map(|identity| identity.uid)
        };
        let Some(feature_uid) = feature_uid else {
            continue;
        };
        anyhow::ensure!(
            item.feature_uid
                .as_deref()
                .is_none_or(|existing| existing == feature_uid),
            "Open item {} is linked to a different feature identity",
            item.id
        );
        item.feature_uid = Some(feature_uid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArtifactIdentity, ItemKind, OpenItem, Priority};

    #[test]
    fn legacy_open_item_gets_one_uid_and_links_to_feature_uid() {
        let root = std::env::temp_dir().join(format!(
            "koolade_open_item_uid_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let dir = root.join(".koolade-packet/planning/changes/F1-feature");
        std::fs::create_dir_all(&dir).unwrap();
        let feature_text = ArtifactIdentity::preserve_markdown(
            "# F1: Feature\n\n**Status:** Draft\n\n## Intent\n\nKeep a stable feature identity.\n",
            None,
            "F1",
            "Feature",
        )
        .unwrap();
        std::fs::write(dir.join("specification.md"), feature_text.clone()).unwrap();
        let feature_uid = ArtifactIdentity::from_markdown(&feature_text)
            .unwrap()
            .unwrap()
            .uid;
        let mut state = PlannerState::load(&root).unwrap();
        let mut item = OpenItem::new(
            "CLR-001".into(),
            Priority::High,
            ItemKind::Question,
            "Product".into(),
            None,
            "Which behavior is expected?".into(),
            "The feature leaves this open.".into(),
        );
        item.uid = None;
        item.feature_id = Some("F1".into());
        state.items.push(item);

        stabilize_open_item_identities(&mut state, &BTreeMap::new()).unwrap();
        let saved_uid = state.items[0].uid.clone().unwrap();
        assert_eq!(
            state.items[0].feature_uid.as_deref(),
            Some(feature_uid.as_str())
        );
        stabilize_open_item_identities(&mut state, &BTreeMap::new()).unwrap();
        assert_eq!(state.items[0].uid.as_deref(), Some(saved_uid.as_str()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
