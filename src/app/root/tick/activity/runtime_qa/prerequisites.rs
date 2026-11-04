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

mod cache_refs;
pub(super) use cache_refs::{available_broker_dependencies, selected_broker_cache_refs};
mod manager_requests;
pub(super) use manager_requests::queue_missing_toolchain_reviews;

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
    if request.mode == crate::runtime_qa::QaRunMode::Experimental {
        let engine = crate::execution::docker::environment::DockerEnvironmentStatus::detect();
        if !experimental_engine_supported(
            cfg!(target_os = "linux"),
            engine.engine_ready,
            engine.rootless,
            engine.architecture.as_deref(),
            engine.desktop_vm,
        ) {
            return Some("experimental Runtime QA requires a ready rootless Linux amd64 Docker Engine; Docker Desktop, rootful Docker, other architectures, and host execution are unsupported.".into());
        }
        let feature = match request.relevant_platforms.as_slice() {
            [
                crate::runtime_qa::QaPlatform::Web | crate::runtime_qa::QaPlatform::HeadlessService,
            ] => crate::persistence::experimental_features::ExperimentalFeature::BrowserQa,
            [crate::runtime_qa::QaPlatform::LinuxDesktop] => {
                crate::persistence::experimental_features::ExperimentalFeature::LinuxDesktopQa
            }
            _ => {
                return Some(
                    "experimental QA request does not name one supported driver profile".into(),
                );
            }
        };
        if !crate::persistence::experimental_features::load()
            .0
            .feature_enabled(feature)
        {
            return Some("Experimental Mode or this QA feature is now OFF. No new run will start; re-enable it and create a fresh reviewed Runtime QA request if dogfooding should continue.".into());
        }
    }
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
    let target_id = request.target_id.as_deref();
    let target = manifest.candidate_targets.iter().find(|target| {
        Some(target.target_id.as_str()) == target_id
            && target.start_recipe_ref.as_deref() == Some(request.startup_recipe_ref.as_str())
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
    let requested_surface = requested_surface(&request.relevant_platforms);
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
    let dependency_cache_store = crate::execution::DependencyCacheSnapshotStore::new(
        crate::persistence::state_root().join("runtime-qa-dependency-caches"),
    );
    let available_dependencies = available_broker_dependencies(
        target,
        task,
        &project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string(),
        task_progress,
        &dependency_cache_store,
    );
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

    let profile = match profile_for_platforms(&request.relevant_platforms) {
        Some(profile) => profile,
        None => {
            return Some(
                "the requested platform set has no single supported app-owned runtime QA driver."
                    .into(),
            );
        }
    };
    let select = |architecture| {
        if request.mode == crate::runtime_qa::QaRunMode::Experimental {
            lock.select_experimental_qa(
                profile,
                architecture,
                &crate::persistence::experimental_features::load().0,
            )
        } else {
            lock.select(profile, architecture)
        }
    };
    let amd64 = select("amd64");
    let arm64 = select("arm64");
    if request
        .runtime_image_digest
        .as_deref()
        .is_some_and(|digest| {
            !amd64.as_ref().is_ok_and(|image| image.digest == digest)
                && !arm64.as_ref().is_ok_and(|image| image.digest == digest)
        })
    {
        return Some("the immutable QA runtime image digest changed after this request was created; issue a fresh reviewed Runtime QA request".into());
    }
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

pub(super) fn experimental_engine_supported(
    host_linux: bool,
    engine_ready: bool,
    rootless: Option<bool>,
    architecture: Option<&str>,
    desktop_vm: bool,
) -> bool {
    host_linux
        && !desktop_vm
        && engine_ready
        && rootless == Some(true)
        && architecture == Some("amd64")
}

fn requested_surface(platforms: &[QaPlatform]) -> Option<AppSurface> {
    match platforms {
        [QaPlatform::Web] => Some(AppSurface::Web),
        [QaPlatform::HeadlessService] => Some(AppSurface::HeadlessService),
        [QaPlatform::LinuxDesktop] => Some(AppSurface::LinuxDesktop),
        _ => None,
    }
}

fn profile_for_platforms(platforms: &[QaPlatform]) -> Option<RuntimeProfile> {
    match platforms {
        [QaPlatform::Web | QaPlatform::HeadlessService] => Some(RuntimeProfile::BrowserQa),
        [QaPlatform::LinuxDesktop] => Some(RuntimeProfile::DesktopQa),
        _ => None,
    }
}

pub(super) fn task_progress_for_target<'a>(
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

pub(super) fn locked_capabilities(lock: &ImageLock) -> (BTreeSet<String>, BTreeSet<String>) {
    let profiles = [
        RuntimeProfile::General,
        RuntimeProfile::Rust,
        RuntimeProfile::Node,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
    ];
    let mut tools = BTreeSet::new();
    let experimental = crate::persistence::experimental_features::load().0;
    for profile in profiles {
        for architecture in ["amd64", "arm64"] {
            if let Ok(image) = lock
                .select(profile, architecture)
                .or_else(|_| lock.select_experimental_qa(profile, architecture, &experimental))
            {
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
        ["amd64", "arm64"].into_iter().any(|architecture| {
            lock.select(*profile, architecture).is_ok()
                || lock
                    .select_experimental_qa(*profile, architecture, &experimental)
                    .is_ok()
        })
    })
    .map(|(id, _)| id.to_owned())
    .collect();
    (tools, drivers)
}

#[cfg(test)]
mod tests;
