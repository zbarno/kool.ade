use crate::ui::planning_board::ViewModel;
use std::collections::{BTreeMap, HashMap, HashSet};

mod planning_work;
pub(super) use planning_work::{planning_work_relations, unique_planning_work_keys};

pub(super) type Map = HashMap<String, HashSet<String>>;

pub(super) type Dependents = HashMap<String, Vec<(String, String, String)>>;

pub(super) fn unique_task_paths(board: &ViewModel) -> HashMap<String, String> {
    let mut paths = HashMap::new();
    let mut ambiguous = HashSet::new();
    for doc in &board.task_documents {
        let Some(identity) = &doc.identity else {
            continue;
        };
        if paths.contains_key(&identity.uid) {
            paths.remove(&identity.uid);
            ambiguous.insert(identity.uid.clone());
        } else if !ambiguous.contains(&identity.uid) {
            paths.insert(identity.uid.clone(), doc.path.clone());
        }
    }
    paths
}

pub(super) fn task_dependents(board: &ViewModel) -> Dependents {
    let unique_paths = unique_task_paths(board);
    board
        .task_documents
        .iter()
        .filter_map(|doc| {
            let uid = doc.identity.as_ref()?.uid.clone();
            if unique_paths.get(&uid) != Some(&doc.path) {
                return None;
            }
            Some((
                doc.path.clone(),
                board
                    .task_documents
                    .iter()
                    .filter_map(|candidate| {
                        let metadata = candidate.metadata.as_ref()?;
                        let identity = candidate.identity.as_ref()?;
                        if unique_paths.get(&identity.uid) != Some(&candidate.path) {
                            return None;
                        }
                        metadata.dependency_uids.contains(&uid).then(|| {
                            (
                                candidate.path.clone(),
                                candidate.title.clone(),
                                task_status(board, &candidate.path),
                            )
                        })
                    })
                    .collect(),
            ))
        })
        .collect()
}

fn task_status(board: &ViewModel, path: &str) -> String {
    let phase = board
        .lifecycle_tasks
        .get(path)
        .copied()
        .map(crate::ui::task_detail::task_phase_label);
    let pr_status = board.lifecycle_pr_statuses.get(path).map(|status| {
        use crate::core::lifecycle::PullRequestPreparationStatus as Pr;
        match status {
            Pr::AwaitingAuthorization => "PR awaiting authorization",
            Pr::Publishing => "PR publishing",
            Pr::NeedsAttention => "PR needs attention",
            Pr::Opened => "PR open",
            Pr::ClosedUnmerged => "PR closed unmerged",
            Pr::Merged => "PR merged",
        }
    });
    match (phase, pr_status) {
        (Some(phase), Some(pr)) => format!("{phase} · {pr}"),
        (Some(phase), None) => phase.to_owned(),
        (None, Some(pr)) => pr.to_owned(),
        (None, None) => "Status not recorded".to_owned(),
    }
}

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

    for issue in &board.setup_attention {
        card_keys.push(issue.id.to_owned());
    }
    for task in &board.setup_tasks {
        card_keys.push(task.id.clone());
        for dependency in &task.dependencies {
            add(
                &mut tags,
                &task.id,
                "setup_prerequisite",
                &dependency.prerequisite_id,
            );
        }
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
    let candidate = if let Some(rest) = directory.strip_prefix("CHG-") {
        let digits = rest.split_once('-')?.0;
        directory.get(..4 + digits.len())?
    } else {
        directory.split_once('-')?.0
    };
    crate::artifacts::product_docs::valid_feature_id(candidate).then_some(candidate)
}

#[cfg(test)]
#[path = "relationships_tests.rs"]
mod tests;
