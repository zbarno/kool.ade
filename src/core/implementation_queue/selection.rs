use crate::{
    artifacts::task_docs::TaskDocument,
    core::implementation::{Implementation, ImplementationStatus, PullRequestState},
};
use std::{collections::BTreeMap, path::Path};

pub fn next_ticket(
    docs: &[TaskDocument],
    states: &BTreeMap<String, Implementation>,
) -> Result<Option<String>, String> {
    next_ready_ticket(docs, states, &Default::default())
}

pub fn next_ready_ticket(
    docs: &[TaskDocument],
    states: &BTreeMap<String, Implementation>,
    excluded: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, String> {
    next_ready_ticket_with_running_scopes(docs, states, excluded, &Default::default())
}

pub fn next_ready_ticket_with_running_scopes(
    docs: &[TaskDocument],
    states: &BTreeMap<String, Implementation>,
    excluded: &std::collections::BTreeSet<String>,
    running: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, String> {
    let mut docs = docs
        .iter()
        .filter(|doc| !doc.path.ends_with("/README.md"))
        .collect::<Vec<_>>();
    docs.sort_by_key(|doc| &doc.path);
    let done = |path: &str| {
        states.get(path).is_some_and(|state| {
            state.status == ImplementationStatus::Completed
                || state.pr_state == Some(PullRequestState::Merged)
        })
    };
    let mut waiting = Vec::new();
    'tasks: for doc in &docs {
        if done(&doc.path) || excluded.contains(&doc.path) {
            continue;
        }
        if states
            .get(&doc.path)
            .is_some_and(|state| state.pr_url.is_some())
        {
            waiting.push(format!(
                "Waiting for the existing PR for {} to merge",
                doc.title
            ));
            continue;
        }
        if states.get(&doc.path).is_some_and(|state| {
            matches!(
                state.status,
                ImplementationStatus::ReadyToPublish
                    | ImplementationStatus::AwaitingApproval
                    | ImplementationStatus::ChangesRequested
            )
        }) {
            waiting.push(format!(
                "{} is verified and waiting for you to publish",
                doc.title
            ));
            continue;
        }
        if let Err(reason) = dependency_waiting_reason(doc, &docs, states) {
            waiting.push(reason);
            continue 'tasks;
        }
        if let Some(reason) = super::scope_conflicts::conflict_for_active_docs(doc, &docs, running)
        {
            waiting.push(reason);
            continue 'tasks;
        }
        return Ok(Some(doc.path.clone()));
    }
    if waiting.is_empty() {
        Ok(None)
    } else {
        Err(waiting.join("\n"))
    }
}

pub fn ticket_readiness(
    docs: &[TaskDocument],
    states: &BTreeMap<String, Implementation>,
    ticket: &str,
) -> Result<(), String> {
    let docs = docs
        .iter()
        .filter(|doc| !doc.path.ends_with("/README.md"))
        .collect::<Vec<_>>();
    let doc = docs
        .iter()
        .find(|doc| doc.path == ticket)
        .ok_or_else(|| format!("Task {ticket} is no longer on the board"))?;
    dependency_waiting_reason(doc, &docs, states)
}

fn dependency_waiting_reason(
    doc: &TaskDocument,
    docs: &[&TaskDocument],
    states: &BTreeMap<String, Implementation>,
) -> Result<(), String> {
    let dependencies = task_dependencies(doc, docs).map_err(|error| {
        format!(
            "{} is waiting because its task dependency metadata is invalid: {error}",
            doc.title
        )
    })?;
    for dependency in dependencies {
        let done = states.get(&dependency).is_some_and(|state| {
            state.status == ImplementationStatus::Completed
                || state.pr_state == Some(PullRequestState::Merged)
        });
        if !done {
            return Err(format!(
                "{} is waiting for dependency {dependency}",
                doc.title
            ));
        }
    }
    Ok(())
}

fn task_dependencies(doc: &TaskDocument, docs: &[&TaskDocument]) -> Result<Vec<String>, String> {
    if let Some(error) = &doc.metadata_error {
        return Err(error.clone());
    }
    if let Some(metadata) = &doc.metadata {
        metadata
            .validate(doc.identity.as_ref())
            .map_err(|error| error.to_string())?;
        return metadata
            .dependency_uids
            .iter()
            .map(|uid| {
                docs.iter()
                    .find(|candidate| candidate.identity.as_ref().is_some_and(|id| &id.uid == uid))
                    .map(|candidate| candidate.path.clone())
                    .ok_or_else(|| format!("dependency identity {uid} is not on the task board"))
            })
            .collect();
    }
    let filenames = crate::artifacts::task_docs::legacy_dependencies(&doc.text)
        .map_err(|error| error.to_string())?;
    let parent = Path::new(&doc.path)
        .parent()
        .ok_or_else(|| "task path has no batch directory".to_string())?;
    filenames
        .into_iter()
        .map(|filename| {
            let target = parent.join(filename);
            let path = target.to_string_lossy().into_owned();
            docs.iter()
                .any(|candidate| candidate.path == path)
                .then_some(path)
                .ok_or_else(|| format!("legacy dependency {target:?} is not on the task board"))
        })
        .collect()
}
