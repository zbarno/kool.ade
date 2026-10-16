use crate::{app::session::Project, core::lifecycle::LifecycleStore};

pub(super) fn for_ticket(
    project: &Project,
    ticket: &str,
) -> (
    Option<crate::ui::task_detail::LifecycleView>,
    Option<String>,
) {
    let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    else {
        return (None, None);
    };
    let Some(planning_item_uid) = document
        .identity
        .as_ref()
        .map(|identity| identity.uid.as_str())
    else {
        return (None, None);
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = match lifecycle.task_for_planning_item(planning_item_uid) {
        Ok(Some(task)) => task,
        Ok(None) => return (None, None),
        Err(error) => return (None, Some(error.to_string())),
    };
    let feature = match lifecycle.load_feature(&task.feature_uid) {
        Ok(feature) => feature,
        Err(error) => return (None, Some(error.to_string())),
    };
    (
        Some(crate::ui::task_detail::LifecycleView {
            feature_phase: feature.phase,
            task_phase: task.phase,
            task_revision: task.revision,
            code_review: task.code_review,
            runtime_qa: task.runtime_qa,
            spec_reconciliation: task.spec_reconciliation,
        }),
        None,
    )
}

pub(in crate::app::root::surface) fn board_phases(
    project: &Project,
) -> (
    std::collections::BTreeMap<String, crate::core::lifecycle::TaskPhase>,
    Option<String>,
) {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let tasks = match lifecycle.list_tasks() {
        Ok(tasks) => tasks,
        Err(error) => return (std::collections::BTreeMap::new(), Some(error.to_string())),
    };
    let mappings = tasks.into_iter().filter_map(|task| {
        task.planning_item_uid
            .map(|planning_item_uid| (planning_item_uid, task.phase))
    });
    let (phases, has_ambiguous) = phase_index(mappings);
    let by_path = project
        .task_documents
        .iter()
        .filter_map(|document| {
            let planning_item_uid = document.identity.as_ref()?.uid.as_str();
            let Some(Some(phase)) = phases.get(planning_item_uid) else {
                return None;
            };
            Some((document.path.clone(), *phase))
        })
        .collect();
    let error = has_ambiguous.then(|| {
        "Lifecycle badges are hidden for planning items with multiple lifecycle task records."
            .to_owned()
    });
    (by_path, error)
}

fn phase_index(
    mappings: impl IntoIterator<Item = (String, crate::core::lifecycle::TaskPhase)>,
) -> (
    std::collections::BTreeMap<String, Option<crate::core::lifecycle::TaskPhase>>,
    bool,
) {
    let mut phases = std::collections::BTreeMap::new();
    let mut ambiguous = false;
    for (planning_item_uid, phase) in mappings {
        match phases.entry(planning_item_uid) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(phase));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(None);
                ambiguous = true;
            }
        }
    }
    (phases, ambiguous)
}

#[cfg(test)]
mod tests {
    use super::phase_index;
    use crate::core::lifecycle::TaskPhase;

    #[test]
    fn duplicate_planning_item_mappings_are_marked_ambiguous() {
        let (phases, ambiguous) = phase_index([
            ("item-1".into(), TaskPhase::AwaitingReview),
            ("item-1".into(), TaskPhase::PrMerged),
        ]);
        assert!(ambiguous);
        assert_eq!(phases.get("item-1"), Some(&None));
    }

    #[test]
    fn distinct_planning_items_keep_their_lifecycle_phase() {
        let (phases, ambiguous) = phase_index([
            ("item-1".into(), TaskPhase::AwaitingReview),
            ("item-2".into(), TaskPhase::PrMerged),
        ]);
        assert!(!ambiguous);
        assert_eq!(phases.get("item-1"), Some(&Some(TaskPhase::AwaitingReview)));
        assert_eq!(phases.get("item-2"), Some(&Some(TaskPhase::PrMerged)));
    }
}
