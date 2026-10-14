use serde::{Deserialize, Serialize};

use super::super::{QaRunRequestV1, is_sha256};

/// Startup details resolved by the app from its trusted recipe registry.
/// Scenario and task request payloads cannot choose executable paths or argv.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaValidatedTargetV1 {
    pub task_uid: String,
    pub build_artifact_sha256: String,
    pub startup_recipe_ref: String,
    pub startup_recipe_sha256: String,
    pub executable: String,
    pub working_directory: String,
    pub arguments: Vec<String>,
}

impl QaValidatedTargetV1 {
    pub fn validate_for(&self, request: &QaRunRequestV1) -> Result<(), String> {
        if self.task_uid != request.parent_task_uid
            || self.build_artifact_sha256 != request.build_artifact_sha256
            || self.startup_recipe_ref != request.startup_recipe_ref
            || self.startup_recipe_sha256 != request.startup_recipe_sha256
            || !is_sha256(&self.startup_recipe_sha256)
            || !valid_workspace_path(&self.executable, false)
            || !valid_workspace_path(&self.working_directory, true)
            || self.arguments.len() > 128
            || self
                .arguments
                .iter()
                .any(|argument| argument.len() > 4096 || argument.contains('\0'))
        {
            return Err(
                "runtime QA launch target is not bound to its approved task build and recipe"
                    .into(),
            );
        }
        Ok(())
    }
}

fn valid_workspace_path(value: &str, allow_root: bool) -> bool {
    if allow_root && value == "/workspace" {
        return true;
    }
    let Some(relative) = value.strip_prefix("/workspace/") else {
        return false;
    };
    !value.contains('\\')
        && !value.contains('\0')
        && !relative.is_empty()
        && relative
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}
