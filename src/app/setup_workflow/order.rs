//! Deterministic sequencing for the setup flow.
//!
//! Walks the persisted prerequisite DAG (the same edges the store validates)
//! with a stable, human-meaningful tie-break so the same machine state always
//! produces the same plan: foundation checks first, container runtime next,
//! coding tools and locked images after, VM and OS-specific work last.

use std::collections::{BTreeMap, BTreeSet};

use crate::persistence::setup_tasks::SetupTaskV1;

#[derive(Debug, Eq, PartialEq)]
pub enum SequenceError {
    Cycle,
}

/// Longest-prefix category ranking; lower ranks run first.
const CATEGORY_RULES: [(&str, u8); 35] = [
    ("setup:linux:rootless:packages", 1),
    ("setup:linux:rootless:subids", 2),
    ("setup:linux:rootless:service", 3),
    ("setup:docker:install", 10),
    ("setup:docker:engine", 11),
    ("setup:docker:daemon", 12),
    ("setup:docker:start", 13),
    ("setup:docker:desktop-transport", 14),
    ("setup:docker:desktop", 15),
    ("setup:docker:endpoint", 16),
    ("setup:docker:permission", 17),
    ("setup:docker:rootless", 18),
    ("setup:docker:cgroups", 19),
    ("setup:docker:seccomp", 20),
    ("setup:docker:os", 21),
    ("setup:docker:profile", 22),
    ("setup:docker:capabilities", 23),
    ("setup:docker:api", 24),
    ("setup:docker:workspace", 25),
    ("setup:docker:disk", 26),
    ("setup:docker:limits", 27),
    ("setup:docker:architecture", 28),
    ("setup:docker:windows-containers", 29),
    ("setup:docker:image-", 30),
    ("setup:git:", 31),
    ("setup:cli:install", 32),
    ("setup:cli:select", 33),
    ("setup:cli:scan", 34),
    ("setup:cli:", 35),
    ("setup:images:general:", 40),
    ("setup:images:", 41),
    ("setup:computer:kvm", 50),
    ("setup:computer:qemu", 51),
    ("setup:computer:", 52),
    ("setup:windows:", 60),
];

fn order_category(id: &str) -> u8 {
    CATEGORY_RULES
        .iter()
        .filter(|(prefix, _)| id.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, rank)| *rank)
        .unwrap_or(70)
}

pub fn sequence(tasks: &[SetupTaskV1]) -> Result<Vec<String>, SequenceError> {
    let ids: BTreeSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    let mut indegree: BTreeMap<&str, usize> = tasks
        .iter()
        .map(|task| (task.id.as_str(), 0usize))
        .collect();
    let mut children: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for task in tasks {
        for prerequisite in &task.blocked_by {
            if ids.contains(prerequisite.as_str()) {
                *indegree
                    .get_mut(task.id.as_str())
                    .expect("every task ID was collected") += 1;
                children
                    .entry(prerequisite.as_str())
                    .or_default()
                    .insert(task.id.as_str());
            }
        }
    }

    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut ordered = Vec::with_capacity(tasks.len());
    while let Some(next) = ready
        .iter()
        .min_by_key(|id| (order_category(id), *id))
        .copied()
    {
        ready.remove(next);
        ordered.push(next.to_owned());
        if let Some(dependents) = children.get(next).cloned() {
            for child in dependents {
                let degree = indegree
                    .get_mut(child)
                    .expect("dependents belong to the task set");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(child);
                }
            }
        }
    }
    if ordered.len() != tasks.len() {
        return Err(SequenceError::Cycle);
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::setup_tasks::State;

    fn task(id: &str, blocked_by: &[&str]) -> SetupTaskV1 {
        SetupTaskV1 {
            schema_version: 1,
            id: id.into(),
            title: String::new(),
            explanation: String::new(),
            next_step: String::new(),
            dependencies: Vec::new(),
            blocked_by: blocked_by.iter().map(|id| id.to_string()).collect(),
            state: State::ReadyToFix,
            created_at_unix_ms: 1,
            observed_at_unix_ms: 1,
            scan_generation: 1,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: Vec::new(),
            superseded_by: Vec::new(),
        }
    }

    #[test]
    fn prerequisites_complete_before_dependent_work() {
        let tasks = vec![
            task("setup:docker:engine", &["setup:linux:rootless:subids"]),
            task(
                "setup:linux:rootless:subids",
                &["setup:linux:rootless:packages"],
            ),
            task("setup:linux:rootless:packages", &[]),
        ];
        assert_eq!(
            sequence(&tasks).unwrap(),
            vec![
                "setup:linux:rootless:packages",
                "setup:linux:rootless:subids",
                "setup:docker:engine"
            ]
        );
    }

    #[test]
    fn categories_tiebreak_within_an_unrelated_set() {
        let tasks = vec![
            task("setup:images:general:x-y", &[]),
            task("setup:linux:rootless:packages", &[]),
            task("setup:docker:engine", &[]),
        ];
        assert_eq!(
            sequence(&tasks).unwrap(),
            vec![
                "setup:linux:rootless:packages",
                "setup:docker:engine",
                "setup:images:general:x-y"
            ]
        );
    }

    #[test]
    fn edges_pointing_outside_the_observed_set_do_not_stall_progress() {
        let tasks = vec![
            task("setup:computer:qemu", &["setup:not-in-this-snapshot"]),
            task("setup:computer:kvm", &[]),
        ];
        assert_eq!(
            sequence(&tasks).unwrap(),
            vec!["setup:computer:kvm", "setup:computer:qemu"]
        );
    }

    #[test]
    fn cycles_are_reported_instead_of_looping_forever() {
        let tasks = vec![
            task("setup:a:first", &["setup:a:second"]),
            task("setup:a:second", &["setup:a:first"]),
        ];
        assert_eq!(sequence(&tasks), Err(SequenceError::Cycle));
    }
}
