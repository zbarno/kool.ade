use crate::ui::planning_board::ViewModel;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) type Map = HashMap<String, HashSet<String>>;

pub(super) fn build(board: &ViewModel) -> Map {
    let mut tags = BTreeMap::<String, Vec<String>>::new();
    let mut card_keys = Vec::new();

    for work in &board.planning_work {
        let key = work.key.clone();
        card_keys.push(key.clone());
        add(&mut tags, &key, "work_uid", &work.uid);
        if let Some(parent) = &work.parent_uid {
            add(&mut tags, &key, "work_uid", parent);
        }
        if let Some(uid) = &work.feature_uid {
            add(&mut tags, &key, "feature_uid", uid);
        }
        if let Some(id) = &work.feature_id {
            add(&mut tags, &key, "feature_id", id);
        }
    }

    for item in &board.planning_items {
        let key = item.id.clone();
        card_keys.push(key.clone());
        add(&mut tags, &key, "item_id", &item.id);
        if let Some(uid) = &item.feature_uid {
            add(&mut tags, &key, "feature_uid", uid);
        }
        if let Some(id) = &item.feature_id {
            add(&mut tags, &key, "feature_id", id);
        }
        for dependency in &item.blocked_by {
            add(&mut tags, &key, "item_id", dependency);
        }
    }

    if let Some(issue) = &board.setup_attention {
        card_keys.push(issue.id.to_owned());
    }

    for doc in &board.task_documents {
        if doc.path.ends_with("/README.md") {
            continue;
        }
        let key = doc.path.clone();
        card_keys.push(key.clone());
        if let Some(metadata) = &doc.metadata {
            add(&mut tags, &key, "task_uid", &metadata.uid);
            add(&mut tags, &key, "batch_uid", &metadata.batch_uid);
            for dependency in &metadata.dependency_uids {
                add(&mut tags, &key, "task_uid", dependency);
            }
        }
        if let Some(identity) = &doc.identity
            && let Some(parent) = &identity.parent_uid
        {
            add(&mut tags, &key, "batch_uid", parent);
        }
        if let Some(feature_id) = feature_id_in_path(&doc.path) {
            add(&mut tags, &key, "feature_id", feature_id);
        }
    }

    let mut related = card_keys
        .into_iter()
        .map(|key| (key, HashSet::new()))
        .collect::<Map>();
    for keys in tags.values() {
        for key in keys {
            if let Some(neighbors) = related.get_mut(key) {
                neighbors.extend(keys.iter().filter(|other| *other != key).cloned());
            }
        }
    }
    related.retain(|_, neighbors| !neighbors.is_empty());
    related
}

fn add(tags: &mut BTreeMap<String, Vec<String>>, key: &str, kind: &str, value: &str) {
    tags.entry(format!("{kind}:{value}"))
        .or_default()
        .push(key.to_owned());
}

fn feature_id_in_path(path: &str) -> Option<&str> {
    let directory = path.split('/').rev().nth(1)?;
    let candidate = directory.split('-').next()?;
    crate::artifacts::product_docs::valid_feature_id(candidate).then_some(candidate)
}

#[cfg(test)]
#[path = "relationships_tests.rs"]
mod tests;
