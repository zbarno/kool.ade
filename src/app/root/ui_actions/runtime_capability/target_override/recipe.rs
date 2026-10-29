use crate::{
    project_runtime::{AppSurface, ExecutionRecipeV1, NetworkRequirement, RecipeSource},
    ui::task_detail::RuntimeRecipeInput,
};

pub(super) fn append_recipe(
    recipes: &mut Vec<ExecutionRecipeV1>,
    operation: &str,
    app_surface: AppSurface,
    input: &RuntimeRecipeInput,
    approval_ref: &str,
    artifact_paths: &[String],
) -> Result<Option<String>, String> {
    let executable_id = input.executable_id.trim();
    let argv = input
        .argv_lines
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let has_selected_env_keys = input
        .allowed_env_keys_lines
        .lines()
        .any(|line| !line.trim().is_empty());
    if executable_id.is_empty() && argv.is_empty() && !has_selected_env_keys {
        return Ok(None);
    }
    if executable_id.is_empty() {
        return Err(format!(
            "The {operation} recipe needs an executable ID or path."
        ));
    }
    if !crate::project_runtime::is_logical_tool_id(executable_id)
        && !crate::project_runtime::is_target_relative_executable_path(executable_id)
    {
        return Err(format!("The {operation} recipe executable is invalid."));
    }
    let is_path = crate::project_runtime::is_target_relative_executable_path(executable_id);
    if operation != "start" && is_path {
        return Err(format!(
            "The {operation} recipe must use a logical tool ID; executable paths are only supported for Start."
        ));
    }
    if argv.len() > 128 || argv.iter().any(|arg| arg.chars().any(char::is_control)) {
        return Err(format!(
            "The {operation} recipe has control characters or too many arguments."
        ));
    }
    let recipe_id = format!("user-{}-{}", operation, uuid::Uuid::new_v4().simple());
    let service_start = operation == "start"
        && matches!(app_surface, AppSurface::Web | AppSurface::HeadlessService);
    let mut allowed_env_keys = if service_start {
        vec!["HOST".into(), "PORT".into()]
    } else {
        Vec::new()
    };
    let mut seen_env_keys = std::collections::BTreeSet::new();
    for key in input
        .allowed_env_keys_lines
        .lines()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        if !seen_env_keys.insert(key) {
            return Err(format!(
                "The {operation} recipe environment allowlist contains a duplicate key."
            ));
        }
        if seen_env_keys.len() > 64 {
            return Err(format!(
                "The {operation} recipe can allow at most 64 environment keys."
            ));
        }
        if !allowed_env_keys.iter().any(|allowed| allowed == key) {
            allowed_env_keys.push(key.to_owned());
        }
        if allowed_env_keys.len() > 64 {
            return Err(format!(
                "The {operation} recipe can allow at most 64 environment keys."
            ));
        }
    }
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.clone(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: executable_id.to_owned(),
        argv,
        allowed_env_keys,
        allowed_output_paths: if operation == "build" {
            artifact_paths.to_vec()
        } else {
            Vec::new()
        },
        timeout_seconds: 1200,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: recipe_prerequisite_tools(operation, app_surface, executable_id),
        network_requirement: if service_start {
            NetworkRequirement::ApprovedServicePorts
        } else {
            NetworkRequirement::Denied
        },
        evidence_refs: vec![approval_ref.to_owned()],
    };
    recipe
        .validate()
        .map_err(|error| format!("The {operation} recipe is invalid: {error}."))?;
    recipes.push(recipe);
    Ok(Some(recipe_id))
}

fn recipe_prerequisite_tools(
    operation: &str,
    app_surface: AppSurface,
    executable_id: &str,
) -> Vec<String> {
    if operation != "start" {
        return vec![executable_id.to_owned()];
    }
    let mut tools = if crate::project_runtime::is_target_relative_executable_path(executable_id) {
        Vec::new()
    } else {
        vec![executable_id.to_owned()]
    };
    // The locked Python profile supplies an application-owned standard-library
    // HTTP probe when a Web/Headless app does not already declare a probe tool.
    if matches!(app_surface, AppSurface::Web | AppSurface::HeadlessService)
        && !tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "node" | "npm" | "python" | "python3" | "go"))
    {
        tools.push("python".into());
    } else if matches!(app_surface, AppSurface::Web | AppSurface::HeadlessService)
        && tools.iter().any(|tool| tool == "npm")
        && !tools.iter().any(|tool| tool == "node")
    {
        tools.push("node".into());
    }
    tools
}
