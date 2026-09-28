use super::{PlannerState, UpdatePatch};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn cycle_errors(
    state: &PlannerState,
    added: &[crate::domain::OpenItem],
    updates: &[(String, UpdatePatch)],
    resolved: &[String],
) -> Vec<String> {
    let replacements = updates
        .iter()
        .filter_map(|(id, patch)| {
            patch
                .blocked_by
                .as_ref()
                .map(|dependencies| (id.clone(), dependencies.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut graph = state
        .items
        .iter()
        .filter(|item| !resolved.contains(&item.id))
        .map(|item| {
            (
                item.id.clone(),
                replacements
                    .get(&item.id)
                    .cloned()
                    .unwrap_or_else(|| item.blocked_by.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    graph.extend(
        added
            .iter()
            .map(|item| (item.id.clone(), item.blocked_by.clone())),
    );

    let mut indegree = graph
        .keys()
        .map(|id| (id.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeMap::<String, BTreeSet<String>>::new();
    for (item, dependencies) in &graph {
        for dependency in dependencies.iter().filter(|id| graph.contains_key(*id)) {
            *indegree.get_mut(item).expect("graph item has indegree") += 1;
            children
                .entry(dependency.clone())
                .or_default()
                .insert(item.clone());
        }
    }
    let mut ready = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(id) = ready.pop_front() {
        visited += 1;
        if let Some(next) = children.get(&id) {
            for child in next {
                let count = indegree.get_mut(child).expect("child has indegree");
                *count -= 1;
                if *count == 0 {
                    ready.push_back(child.clone());
                }
            }
        }
    }
    if visited == graph.len() {
        Vec::new()
    } else {
        vec![
            "board-item dependency cycle detected; prerequisite relationships must be acyclic"
                .into(),
        ]
    }
}
