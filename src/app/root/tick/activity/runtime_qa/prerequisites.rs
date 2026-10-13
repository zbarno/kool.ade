use crate::{
    app::session::Project,
    execution::docker::images::{ImageLock, RuntimeProfile},
    project_runtime::{AppSurface, ProjectRuntimeStore, RecipeSource},
    runtime_qa::QaPlatform,
};

pub(super) fn event_uid(task_uid: &str, revision: u64, label: &str) -> String {
    let namespace = uuid::Uuid::parse_str(task_uid).unwrap_or(uuid::Uuid::NAMESPACE_OID);
    uuid::Uuid::new_v5(&namespace, format!("{label}:{revision}").as_bytes())
        .hyphenated()
        .to_string()
}

pub(super) fn unavailable_reason(
    project: &Project,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> Option<String> {
    let manifest = match ProjectRuntimeStore::new(&project.state.planning_store).load_manifest() {
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
            && match request.relevant_platforms.as_slice() {
                [QaPlatform::Web] => target.app_surface == AppSurface::Web,
                [QaPlatform::LinuxDesktop] => target.app_surface == AppSurface::LinuxDesktop,
                _ => false,
            }
    });
    let Some(_target) = target else {
        return Some(
            "the request's startup recipe is not attached to a compatible manifest target; select a matching approved target and issue a fresh QA request.".into(),
        );
    };
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
    if recipe.source == RecipeSource::ValidatedProjectConfig
        || (recipe.source == RecipeSource::UserApproved
            && !recipe
                .evidence_refs
                .iter()
                .any(|reference| reference.starts_with("user-approval:")))
    {
        return Some(
            "the startup recipe lacks an explicit user approval or trusted-adapter identity; approve a supported recipe before runtime QA.".into(),
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
    let lock = match ImageLock::bundled() {
        Ok(lock) => lock,
        Err(error) => {
            return Some(format!(
                "the bundled runtime image lock is invalid ({}); repair the lock before retrying QA.",
                error.message
            ));
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
