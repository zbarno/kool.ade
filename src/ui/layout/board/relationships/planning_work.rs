use std::collections::{HashMap, HashSet};

pub(super) type PlanningWorkRelations =
    HashMap<String, Vec<(String, String, String, String, Option<String>)>>;

pub fn unique_planning_work_keys(
    work: &[crate::core::planning_work::Work],
) -> HashMap<String, String> {
    let mut keys = HashMap::new();
    let mut ambiguous = HashSet::new();
    for item in work {
        if keys.contains_key(&item.uid) {
            keys.remove(&item.uid);
            ambiguous.insert(item.uid.clone());
        } else if !ambiguous.contains(&item.uid) {
            keys.insert(item.uid.clone(), item.key.clone());
        }
    }
    keys
}

pub fn unique_feature_uids_by_id(
    work: &[crate::core::planning_work::Work],
) -> HashMap<String, String> {
    let mut feature_uids = HashMap::new();
    let mut ambiguous = HashSet::new();
    for item in work {
        let (Some(feature_id), Some(feature_uid)) =
            (item.feature_id.as_ref(), item.feature_uid.as_ref())
        else {
            continue;
        };
        if feature_uids
            .get(feature_id)
            .is_some_and(|existing| existing != feature_uid)
        {
            feature_uids.remove(feature_id);
            ambiguous.insert(feature_id.clone());
        } else if !ambiguous.contains(feature_id) {
            feature_uids.insert(feature_id.clone(), feature_uid.clone());
        }
    }
    feature_uids
}

pub fn planning_work_relations(work: &[crate::core::planning_work::Work]) -> PlanningWorkRelations {
    let mut relations = PlanningWorkRelations::new();
    for anchor in work.iter().filter_map(|work| work.feature_uid.as_deref()) {
        let mut related = HashSet::new();
        for item in work
            .iter()
            .filter(|item| item.feature_uid.as_deref() == Some(anchor))
        {
            related.insert(item.uid.clone());
        }
        loop {
            let before = related.len();
            for item in work {
                if item
                    .parent_uid
                    .as_ref()
                    .is_some_and(|parent| related.contains(parent))
                    || related.contains(&item.uid)
                        && item.parent_uid.as_ref().is_some_and(|parent| {
                            work.iter().any(|candidate| candidate.uid == *parent)
                        })
                {
                    related.insert(item.uid.clone());
                    if let Some(parent) = &item.parent_uid {
                        related.insert(parent.clone());
                    }
                }
            }
            if related.len() == before {
                break;
            }
        }
        let mut seen = HashSet::new();
        let records = work
            .iter()
            .filter(|item| related.contains(&item.uid) && seen.insert(item.uid.clone()))
            .map(|item| {
                (
                    item.uid.clone(),
                    item.title.clone(),
                    item.kind.label().to_owned(),
                    format!("{:?}", item.status),
                    item.parent_uid.clone(),
                )
            })
            .collect();
        relations.insert(anchor.to_owned(), records);
    }
    relations
}
