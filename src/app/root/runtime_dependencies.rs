use crate::{
    core::lifecycle::TaskRecord,
    execution::{DependencyCacheSnapshotStore, contract::DependencyCacheRefV1},
    harness::{DependencyRequest, DependencyRequestStatus, LiveProgress},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(in crate::app::root) struct PreparedBrokerDependency {
    pub(in crate::app::root) request_id: String,
    pub(in crate::app::root) label: String,
    pub(in crate::app::root) cache_refs: Vec<DependencyCacheRefV1>,
}

/// Returns only same-task broker preparations whose retained cache snapshot
/// still matches its task, project, ecosystem, and lockfile identity.
pub(in crate::app::root) fn prepared_broker_dependencies(
    task: &TaskRecord,
    project_id: &str,
    progress: Option<&LiveProgress>,
) -> Vec<PreparedBrokerDependency> {
    let Some(progress) = progress else {
        return Vec::new();
    };
    let mut selected = BTreeMap::new();
    for request in &progress.dependency_requests {
        if !matches!(request.status, DependencyRequestStatus::Prepared)
            || task.planning_item_uid.as_deref() != Some(request.task_id.as_str())
        {
            continue;
        }
        let Some(preparation) = request.preparation.as_ref().filter(|preparation| {
            preparation.status == Some(crate::harness::DependencyPreparationStatus::Prepared)
        }) else {
            continue;
        };
        let Some(lockfile_identity) = request.need.lockfile_identity.as_deref() else {
            continue;
        };
        let cache_refs =
            distinct_arch_cache_refs(preparation.cache_refs.iter().rev().filter(|cache_ref| {
                valid_cache_ref(request, task, project_id, lockfile_identity, cache_ref)
            }));
        if !cache_refs.is_empty() {
            selected.insert(
                request.id.clone(),
                PreparedBrokerDependency {
                    request_id: request.id.clone(),
                    label: dependency_label(request),
                    cache_refs,
                },
            );
        }
    }
    selected.into_values().collect()
}

fn distinct_arch_cache_refs<'a>(
    cache_refs: impl Iterator<Item = &'a DependencyCacheRefV1>,
) -> Vec<DependencyCacheRefV1> {
    let mut architectures = BTreeSet::new();
    cache_refs
        .filter(|cache_ref| {
            crate::execution::docker::discovery::parse_architecture(&cache_ref.target_arch)
                .is_some_and(|architecture| architectures.insert(architecture))
        })
        .cloned()
        .collect()
}

fn valid_cache_ref(
    request: &DependencyRequest,
    task: &TaskRecord,
    project_id: &str,
    lockfile_identity: &str,
    cache_ref: &DependencyCacheRefV1,
) -> bool {
    let ecosystem = match request.need.ecosystem {
        crate::harness::PackageEcosystem::Npm => {
            crate::execution::contract::DependencyCacheEcosystemV1::Npm
        }
        crate::harness::PackageEcosystem::Cargo => {
            crate::execution::contract::DependencyCacheEcosystemV1::Cargo
        }
        crate::harness::PackageEcosystem::Pip => {
            crate::execution::contract::DependencyCacheEcosystemV1::Pip
        }
        crate::harness::PackageEcosystem::Go => {
            crate::execution::contract::DependencyCacheEcosystemV1::Go
        }
        _ => return false,
    };
    cache_ref.validate().is_ok()
        && task.planning_item_uid.as_deref() == Some(request.task_id.as_str())
        && cache_ref.task_id == request.task_id
        && cache_ref.project_id == project_id
        && cache_ref.target_id == "local"
        && cache_ref.resolved_integrity == lockfile_identity
        && cache_ref.ecosystem == ecosystem
}

fn dependency_label(request: &DependencyRequest) -> String {
    let ecosystem = match request.need.ecosystem {
        crate::harness::PackageEcosystem::Npm => "npm",
        crate::harness::PackageEcosystem::Pnpm => "pnpm",
        crate::harness::PackageEcosystem::Yarn => "Yarn",
        crate::harness::PackageEcosystem::Cargo => "Cargo",
        crate::harness::PackageEcosystem::Nuget => "NuGet",
        crate::harness::PackageEcosystem::Pip => "pip",
        crate::harness::PackageEcosystem::Go => "Go modules",
        crate::harness::PackageEcosystem::Uv => "uv",
        crate::harness::PackageEcosystem::Poetry => "Poetry",
        crate::harness::PackageEcosystem::System => "system",
        crate::harness::PackageEcosystem::Other => "other",
    };
    let package = request
        .need
        .package
        .as_deref()
        .unwrap_or("prepared package set");
    let safe_name = safe_label_value(package, 80);
    let version = request
        .need
        .version
        .as_deref()
        .map(|version| safe_label_value(version, 40))
        .filter(|version| !version.is_empty())
        .map(|version| format!("@{version}"))
        .unwrap_or_default();
    format!("{ecosystem}: {safe_name}{version}")
}

fn safe_label_value(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

/// Load the prepared dependency options shown for a task target. A failed
/// private cache read fails closed by omitting that option.
pub(in crate::app::root) fn prepared_dependencies_for_ticket(
    project: &crate::app::session::Project,
    ticket: &str,
) -> Vec<PreparedBrokerDependency> {
    let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    else {
        return Vec::new();
    };
    let Some(task_id) = document
        .metadata
        .as_ref()
        .map(|metadata| metadata.uid.as_str())
    else {
        return Vec::new();
    };
    let Ok(task) = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
        .load_task(task_id)
    else {
        return Vec::new();
    };
    let project_id = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let progress = project.activity.tasks.get(ticket);
    let candidates = prepared_broker_dependencies(&task, &project_id, progress);
    let store = DependencyCacheSnapshotStore::new(
        crate::persistence::state_root().join("runtime-qa-dependency-caches"),
    );
    candidates
        .into_iter()
        .filter_map(|mut candidate| {
            candidate
                .cache_refs
                .retain(|cache_ref| store.get(&candidate.request_id, cache_ref).is_ok());
            (!candidate.cache_refs.is_empty()).then_some(candidate)
        })
        .collect()
}

/// Load only selected, intact cache snapshots from the task's prepared broker
/// inventory. The target stores opaque request IDs, never package names or
/// paths supplied by a recipe.
pub(in crate::app::root) fn prepared_dependency_cache_refs_for_ticket(
    project: &crate::app::session::Project,
    ticket: &str,
    required_ids: &[String],
) -> Result<Vec<(String, DependencyCacheRefV1)>, String> {
    if required_ids.is_empty() {
        return Ok(Vec::new());
    }
    let prepared = prepared_dependencies_for_ticket(project, ticket);
    let prepared_ids = prepared
        .iter()
        .map(|dependency| dependency.request_id.clone())
        .collect::<Vec<_>>();
    let selected_ids = validate_selected_dependency_ids(required_ids, &prepared_ids)?;
    let mut cache_refs = Vec::new();
    for request_id in selected_ids {
        let dependency = prepared
            .iter()
            .find(|dependency| dependency.request_id == request_id)
            .ok_or_else(|| "selected target dependency is no longer prepared".to_owned())?;
        if dependency.cache_refs.is_empty() {
            return Err("selected target dependency cache is unavailable".into());
        }
        cache_refs.extend(
            dependency
                .cache_refs
                .iter()
                .cloned()
                .map(|cache_ref| (request_id.clone(), cache_ref)),
        );
    }
    Ok(cache_refs)
}

pub(in crate::app::root) fn validate_selected_dependency_ids(
    required: &[String],
    prepared: &[String],
) -> Result<Vec<String>, String> {
    let selected = required
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let available = prepared
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if required
        .iter()
        .chain(prepared.iter())
        .any(|id| crate::execution::contract::validate_dependency_request_id(id).is_err())
        || selected.len() != required.len()
        || selected.len() > 128
        || selected.iter().any(|id| !available.contains(id.as_str()))
    {
        return Err(
            "runtime target dependency is not a valid, intact, prepared broker request".into(),
        );
    }
    Ok(selected.into_iter().collect())
}

#[cfg(test)]
#[path = "runtime_dependencies/tests.rs"]
mod tests;
