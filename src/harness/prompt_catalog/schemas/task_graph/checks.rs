use super::{TaskDraftV1, TaskGraphV1};
use std::collections::{HashMap, HashSet};

pub(super) fn reject_parallel_path_conflicts(task: &TaskDraftV1) -> Result<(), String> {
    let child_indexes = task
        .subtasks
        .iter()
        .enumerate()
        .map(|(index, child)| (child.uid.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut edges = vec![Vec::new(); task.subtasks.len()];
    for (index, child) in task.subtasks.iter().enumerate() {
        for dependency in &child.depends_on {
            if let Some(&dependency_index) = child_indexes.get(dependency.as_str()) {
                edges[dependency_index].push(index);
            }
        }
    }
    for (index, child) in task.subtasks.iter().enumerate() {
        for (other_index, other) in task.subtasks.iter().enumerate().skip(index + 1) {
            if (child.parallelizable && other.parallelizable)
                && paths_overlap(&child.affected_paths, &other.affected_paths)
                && !reachable(&edges, index, other_index)
                && !reachable(&edges, other_index, index)
            {
                return Err(format!(
                    "parallel subtasks {} and {} have overlapping affected paths",
                    child.uid, other.uid
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn reject_unordered_task_path_conflicts(
    graph: &TaskGraphV1,
    indexes: &HashMap<&str, usize>,
    edges: &[Vec<usize>],
) -> Result<(), String> {
    for (index, task) in graph.tasks.iter().enumerate() {
        for other in &graph.tasks[index + 1..] {
            if task.repository_id == other.repository_id
                && paths_overlap(&task.relevant_files, &other.relevant_files)
                && !reachable(
                    edges,
                    indexes[task.uid.as_str()],
                    indexes[other.uid.as_str()],
                )
                && !reachable(
                    edges,
                    indexes[other.uid.as_str()],
                    indexes[task.uid.as_str()],
                )
            {
                return Err(format!(
                    "tasks {} and {} overlap paths but have no dependency ordering",
                    task.uid, other.uid
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn reject_contradictory_sources(tasks: &[TaskDraftV1]) -> Result<(), String> {
    let mut sources = HashMap::new();
    for task in tasks {
        let key = (
            &task.repository_id,
            &task.source_branch,
            &task.destination_branch,
        );
        if let Some(previous) = sources.insert(key, task.source_sha.as_deref())
            && previous != task.source_sha.as_deref()
        {
            return Err(
                "tasks targeting the same repository and branch pair disagree on source SHA".into(),
            );
        }
    }
    Ok(())
}

pub(super) fn cyclic(edges: &[Vec<usize>], indegree: &mut [usize]) -> bool {
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(index) = ready.pop() {
        visited += 1;
        for dependent in &edges[index] {
            indegree[*dependent] -= 1;
            if indegree[*dependent] == 0 {
                ready.push(*dependent);
            }
        }
    }
    visited != edges.len()
}

pub(super) fn reachable(edges: &[Vec<usize>], start: usize, target: usize) -> bool {
    let mut stack = vec![start];
    let mut seen = HashSet::new();
    while let Some(index) = stack.pop() {
        if index == target {
            return true;
        }
        if seen.insert(index) {
            stack.extend(edges[index].iter().copied());
        }
    }
    false
}

pub(super) fn paths_overlap(left: &[String], right: &[String]) -> bool {
    left.iter().any(|path| {
        right.iter().any(|other| {
            path == other
                || path.starts_with(&format!("{other}/"))
                || other.starts_with(&format!("{path}/"))
        })
    })
}

pub(super) fn bounded_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

pub(super) fn bounded_text_list(values: &[String], max_items: usize, item_limit: usize) -> bool {
    values.len() <= max_items && values.iter().all(|value| bounded_text(value, item_limit))
}

pub(super) fn unique_texts(values: &[String], item_limit: usize) -> bool {
    bounded_text_list(values, 512, item_limit)
        && values.iter().collect::<HashSet<_>>().len() == values.len()
}

pub(super) fn bounded_paths(paths: &[String], max_items: usize) -> bool {
    paths.len() <= max_items
        && paths.iter().all(|path| {
            bounded_text(path, 512)
                && !path.starts_with('/')
                && !path.contains('\\')
                && !path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
        })
        && paths.iter().collect::<HashSet<_>>().len() == paths.len()
}

pub(super) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

pub(super) fn valid_repository_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(super) fn valid_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn valid_git_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && !value.starts_with('-')
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("@{")
        && !value
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
        && !value
            .bytes()
            .any(|byte| byte <= b' ' || b"~^:?*[\\".contains(&byte))
}
