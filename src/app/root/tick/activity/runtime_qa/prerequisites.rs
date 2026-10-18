use crate::{
    app::session::Project,
    core::lifecycle::TaskRecord,
    execution::docker::images::{ImageLock, RuntimeProfile},
    project_runtime::{AppSurface, ProjectRuntimeStore},
    project_runtime_capability::{
        ApprovedCapabilityInventory, CapabilityGap, assess_manifest_target,
    },
    runtime_qa::QaPlatform,
};
use std::collections::BTreeSet;

use super::request::verify_provenance_for_request;

pub(super) fn event_uid(task_uid: &str, revision: u64, label: &str) -> String {
    let namespace = uuid::Uuid::parse_str(task_uid).unwrap_or(uuid::Uuid::NAMESPACE_OID);
    uuid::Uuid::new_v5(&namespace, format!("{label}:{revision}").as_bytes())
        .hyphenated()
        .to_string()
}

pub(super) fn unavailable_reason(
    project: &Project,
    task: &TaskRecord,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> Option<String> {
    let runtime = ProjectRuntimeStore::new(&project.state.planning_store);
    let manifest = match runtime.load_manifest() {
        Ok(Some(manifest)) => manifest,
        Ok(None) => {
            return Some(
                "the project has no persisted capability manifest; discover the target and approve its build, test, start, and readiness recipes before retrying QA.".into(),
            );
        }
        Err(error) => {
            return Some(format!(
                "the persisted project capability manifest could not be validated ({error}); repair the manifest before retrying QA."
            ));
        }
    };
    let target = manifest.candidate_targets.iter().find(|target| {
        target.start_recipe_ref.as_deref() == Some(request.startup_recipe_ref.as_str())
            && task.planning_item_uid.as_deref() == Some(target.owning_task_id.as_str())
    });
    let Some(target) = target else {
        return Some(
            "the request's startup recipe is not attached to a compatible manifest target; select a matching approved target and issue a fresh QA request.".into(),
        );
    };
    if task.task_uid != request.parent_task_uid
        || manifest.source_sha != request.code_head_sha
        || task.planning_item_uid.as_deref() != Some(target.owning_task_id.as_str())
    {
        return Some(
            "the runtime capability manifest is not bound to this lifecycle task and reviewed source revision; issue a fresh reviewed QA request.".into(),
        );
    }
    if !manifest.is_active_target(&target.owning_task_id, &target.target_id) {
        return Some(
            "this task has multiple project runtime targets; explicitly select the target that owns the approved QA recipe before retrying.".into(),
        );
    }
    let runtime = ProjectRuntimeStore::new(&project.state.planning_store);
    if let Err(error) = verify_provenance_for_request(&runtime, &manifest, target) {
        return Some(format!(
            "the selected target no longer has current operator-local approval for its build and start recipes ({error}); approve the exact recipes again before runtime QA."
        ));
    }
    let requested_surface = match request.relevant_platforms.as_slice() {
        [QaPlatform::Web] => Some(AppSurface::Web),
        [QaPlatform::LinuxDesktop] => Some(AppSurface::LinuxDesktop),
        _ => None,
    };
    if requested_surface != Some(target.app_surface) {
        let gap = CapabilityGap::UnsupportedSurface(format!("{:?}", target.app_surface));
        return Some(format!(
            "the selected target cannot use the requested visual QA driver. {}",
            gap.remediation()
        ));
    }
    let lock = match ImageLock::bundled() {
        Ok(lock) => lock,
        Err(error) => {
            return Some(format!(
                "the bundled runtime image lock is invalid ({}); repair the lock before retrying QA.",
                error.message
            ));
        }
    };
    let (tools, drivers) = locked_capabilities(&lock);
    let task_progress =
        task_progress_for_target(&project.task_documents, &project.activity.tasks, target);
    let available_dependencies = available_broker_dependencies(target, task, task_progress);
    let inventory = ApprovedCapabilityInventory {
        manifest: &manifest,
        available_tool_ids: &tools,
        available_dependency_ids: &available_dependencies,
        available_driver_ids: &drivers,
    };
    let assessment = match assess_manifest_target(
        &manifest,
        &target.target_id,
        task.planning_item_uid.as_deref().unwrap_or_default(),
        &inventory,
    ) {
        Ok(assessment) => assessment,
        Err(error) => {
            return Some(format!(
                "the selected target capability could not be assessed ({error}); validate its task and source binding before retrying QA."
            ));
        }
    };
    if !assessment.report.gaps.is_empty() {
        let remediation = assessment
            .report
            .gaps
            .iter()
            .map(|gap| gap.remediation())
            .collect::<Vec<_>>()
            .join(" ");
        return Some(format!(
            "target `{}` is {:?} for runtime QA. {remediation}",
            assessment.report.target_id, assessment.report.qa_applicability
        ));
    }
    let recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| recipe.recipe_id == request.startup_recipe_ref);
    let Some(recipe) = recipe else {
        return Some(
            "the manifest target's startup recipe is missing; restore or approve that exact recipe before retrying QA.".into(),
        );
    };
    if !runtime.recipe_is_locally_approved(&manifest, target, recipe) {
        return Some(
            "the startup recipe has no matching local approval record; approve that exact recipe before runtime QA.".into(),
        );
    }

    let profile = match request.relevant_platforms.as_slice() {
        [QaPlatform::Web] => RuntimeProfile::BrowserQa,
        [QaPlatform::LinuxDesktop] => RuntimeProfile::DesktopQa,
        _ => {
            return Some(
                "the requested platform set has no single supported app-owned runtime QA driver."
                    .into(),
            );
        }
    };
    let amd64 = lock.select(profile, "amd64");
    let arm64 = lock.select(profile, "arm64");
    if amd64.is_err() && arm64.is_err() {
        let error = amd64.err().or_else(|| arm64.err()).expect("both failed");
        return Some(format!(
            "the approved {} QA image is unavailable for both supported architectures: {}. Promote and review the image digest, then retry QA.",
            match profile {
                RuntimeProfile::BrowserQa => "browser",
                RuntimeProfile::DesktopQa => "desktop",
                _ => unreachable!(),
            },
            error.message
        ));
    }
    None
}

fn task_progress_for_target<'a>(
    documents: &'a [crate::artifacts::task_docs::TaskDocument],
    activities: &'a std::collections::BTreeMap<String, crate::harness::LiveProgress>,
    target: &crate::project_runtime::AppTargetV1,
) -> Option<&'a crate::harness::LiveProgress> {
    let document = documents.iter().find(|document| {
        document
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.uid == target.owning_task_id)
    })?;
    activities.get(&document.path)
}

/// A manifest dependency reference is the ID of a broker request explicitly
/// attached to this target. Authorization alone is insufficient: the broker
/// must report successful preparation for this task before QA can use it.
fn available_broker_dependencies(
    target: &crate::project_runtime::AppTargetV1,
    task: &TaskRecord,
    progress: Option<&crate::harness::LiveProgress>,
) -> BTreeSet<String> {
    let Some(progress) = progress else {
        return BTreeSet::new();
    };
    progress
        .dependency_requests
        .iter()
        .filter(|request| {
            target.required_dependencies.contains(&request.id)
                && (request.task_id == task.task_uid
                    || task.planning_item_uid.as_deref() == Some(request.task_id.as_str()))
                && request.status == crate::harness::DependencyRequestStatus::Prepared
                && request.preparation.as_ref().is_some_and(|preparation| {
                    matches!(
                        preparation.status,
                        Some(
                            crate::harness::DependencyPreparationStatus::Prepared
                                | crate::harness::DependencyPreparationStatus::AlreadyAvailable
                        )
                    )
                })
        })
        .map(|request| request.id.clone())
        .collect()
}

fn locked_capabilities(lock: &ImageLock) -> (BTreeSet<String>, BTreeSet<String>) {
    let profiles = [
        RuntimeProfile::General,
        RuntimeProfile::Rust,
        RuntimeProfile::Node,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
    ];
    let mut tools = BTreeSet::new();
    for profile in profiles {
        for architecture in ["amd64", "arm64"] {
            if let Ok(image) = lock.select(profile, architecture) {
                tools.extend(
                    image
                        .tool_versions
                        .into_keys()
                        .filter(|tool| !matches!(tool.as_str(), "base" | "shell")),
                );
            }
        }
    }
    let drivers = [
        ("web-driver", RuntimeProfile::BrowserQa),
        ("linux-desktop-driver", RuntimeProfile::DesktopQa),
    ]
    .into_iter()
    .filter(|(_, profile)| {
        ["amd64", "arm64"]
            .into_iter()
            .any(|architecture| lock.select(*profile, architecture).is_ok())
    })
    .map(|(id, _)| id.to_owned())
    .collect();
    (tools, drivers)
}

#[cfg(test)]
mod tests;
