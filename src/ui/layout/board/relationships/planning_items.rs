use std::collections::{HashMap, HashSet};

pub(super) type PlanningItemRelations =
    HashMap<String, Vec<crate::ui::task_detail::PlanningItemRelationView>>;

pub fn unique_planning_item_keys(
    items: &[crate::domain::item::OpenItem],
) -> HashMap<String, String> {
    let mut keys = HashMap::new();
    let mut ambiguous = HashSet::new();
    for item in items {
        if keys.contains_key(&item.id) {
            keys.remove(&item.id);
            ambiguous.insert(item.id.clone());
        } else if !ambiguous.contains(&item.id) {
            keys.insert(item.id.clone(), item.id.clone());
        }
    }
    keys
}

pub fn planning_item_relations(
    items: &[crate::domain::item::OpenItem],
    feature_uids_by_id: &HashMap<String, String>,
    answer_saved: impl Fn(&crate::domain::item::OpenItem) -> bool,
) -> PlanningItemRelations {
    let mut relations = PlanningItemRelations::new();
    let item_ids = items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let feature_id_uid = |item: &crate::domain::item::OpenItem| {
        item.feature_id
            .as_ref()
            .and_then(|feature_id| feature_uids_by_id.get(feature_id))
    };
    let mut anchors = HashSet::new();
    for item in items {
        anchors.extend(item.feature_uid.iter().cloned());
        anchors.extend(feature_id_uid(item).cloned());
    }
    for anchor in anchors {
        let mut related = items
            .iter()
            .filter(|item| {
                item.feature_uid.as_deref() == Some(anchor.as_str())
                    || feature_id_uid(item).is_some_and(|uid| uid == &anchor)
            })
            .map(|item| item.id.clone())
            .collect::<HashSet<_>>();
        loop {
            let before = related.len();
            for item in items {
                if related.contains(&item.id)
                    || item.blocked_by.iter().any(|id| related.contains(id))
                {
                    related.insert(item.id.clone());
                    related.extend(
                        item.blocked_by
                            .iter()
                            .filter(|id| item_ids.contains(id.as_str()))
                            .cloned(),
                    );
                }
            }
            if related.len() == before {
                break;
            }
        }
        let mut seen = HashSet::new();
        let records = items
            .iter()
            .filter(|item| related.contains(&item.id) && seen.insert(item.id.clone()))
            .map(|item| {
                let status = match (item.status, answer_saved(item), item.authority) {
                    (crate::domain::item::ItemStatus::Resolved, _, _) => "Resolved",
                    (crate::domain::item::ItemStatus::Open, true, _) => "Answer saved",
                    (
                        crate::domain::item::ItemStatus::Open,
                        false,
                        crate::domain::item::Authority::Agent,
                    ) => "Running investigation",
                    (
                        crate::domain::item::ItemStatus::Open,
                        false,
                        crate::domain::item::Authority::Review,
                    ) => "Needs user review",
                    (
                        crate::domain::item::ItemStatus::Open,
                        false,
                        crate::domain::item::Authority::Human,
                    ) => "Needs user input",
                };
                crate::ui::task_detail::PlanningItemRelationView {
                    planning_item_id: item.id.clone(),
                    title: item.question.clone(),
                    kind: item.kind.label().to_owned(),
                    status: status.to_owned(),
                    blocked_by: item.blocked_by.clone(),
                    legacy_feature_link: item.feature_uid.is_none()
                        && feature_id_uid(item).is_some_and(|uid| uid == &anchor),
                    feature_reference_conflict: item.feature_uid.as_ref().is_some_and(|uid| {
                        feature_id_uid(item).is_some_and(|mapped_uid| mapped_uid != uid)
                    }) && (item.feature_uid.as_deref()
                        == Some(anchor.as_str())
                        || feature_id_uid(item).is_some_and(|uid| uid == &anchor)),
                }
            })
            .collect();
        relations.insert(anchor, records);
    }
    relations
}
