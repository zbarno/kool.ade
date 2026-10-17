use serde::{Deserialize, Serialize};

const MAX_ARGS: usize = 128;
const MAX_ARG_BYTES: usize = 4096;
const MAX_TIMEOUT_SECS: u64 = 1200;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeSource {
    ValidatedProjectConfig,
    UserApproved,
    TrustedAdapter,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkRequirement {
    Denied,
    ResourceBrokered,
    ApprovedServicePorts,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecipeV1 {
    pub schema_version: u16,
    pub recipe_id: String,
    pub source: RecipeSource,
    pub working_directory_relative: String,
    /// A logical tool name resolved by the selected, approved executor image.
    pub executable_id: String,
    pub argv: Vec<String>,
    pub allowed_env_keys: Vec<String>,
    pub allowed_output_paths: Vec<String>,
    pub timeout_seconds: u64,
    pub expected_exit_codes: Vec<i32>,
    pub expected_artifacts: Vec<String>,
    pub cache_scope: Option<String>,
    pub prerequisite_tool_ids: Vec<String>,
    pub network_requirement: NetworkRequirement,
    pub evidence_refs: Vec<String>,
}

impl ExecutionRecipeV1 {
    pub fn is_explicitly_user_approved(&self) -> bool {
        self.source == RecipeSource::UserApproved
            && self.evidence_refs.iter().any(|reference| {
                reference
                    .strip_prefix("user-approval:")
                    .is_some_and(|receipt| !receipt.trim().is_empty())
            })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || !valid_id(&self.recipe_id) {
            return Err("recipe version or identifier is invalid".into());
        }
        validate_relative_path(&self.working_directory_relative)?;
        if !valid_executable_id(&self.executable_id) {
            return Err("recipe executable must be a logical tool identifier".into());
        }
        if self.argv.is_empty()
            || self.argv.len() > MAX_ARGS
            || self.argv.iter().any(|arg| {
                arg.len() > MAX_ARG_BYTES || arg.contains('\0') || arg == "-c" || arg == "/c"
            })
        {
            return Err("recipe arguments are invalid or request shell command evaluation".into());
        }
        if self.timeout_seconds == 0 || self.timeout_seconds > MAX_TIMEOUT_SECS {
            return Err("recipe timeout is outside the bounded execution limit".into());
        }
        if self.expected_exit_codes.is_empty() || self.expected_exit_codes.len() > 16 {
            return Err("recipe must declare a bounded set of expected exit codes".into());
        }
        for key in &self.allowed_env_keys {
            if !valid_env_key(key) {
                return Err("recipe environment allowlist contains a protected key".into());
            }
        }
        for path in self
            .allowed_output_paths
            .iter()
            .chain(self.expected_artifacts.iter())
        {
            validate_relative_path(path)?;
            if path.split('/').any(|part| part == ".git") {
                return Err("recipe output cannot target Git metadata".into());
            }
        }
        if self.evidence_refs.len() > 64
            || self.evidence_refs.iter().any(|reference| {
                reference.trim().is_empty() || reference.len() > 2048 || reference.contains('\0')
            })
        {
            return Err("recipe evidence references are invalid".into());
        }
        if self.source == RecipeSource::UserApproved && !self.is_explicitly_user_approved() {
            return Err("user-approved recipes must reference an explicit approval receipt".into());
        }
        Ok(())
    }
}

pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(super) fn valid_executable_id(value: &str) -> bool {
    valid_id(value) && !value.contains('/') && !matches!(value, "sh" | "bash" | "cmd" | "pwsh")
}

pub(crate) fn validate_relative_path(value: &str) -> Result<(), String> {
    let path = std::path::Path::new(value);
    if value.is_empty()
        || value.len() > 1024
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
        || value.contains('\0')
    {
        return Err("recipe path must remain relative to the approved workspace".into());
    }
    Ok(())
}

pub(crate) fn validate_custom_target_root(value: &str) -> Result<(), &'static str> {
    if validate_relative_path(value).is_err()
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err("Choose a repository-relative app root without traversal components.");
    }
    Ok(())
}

pub(crate) fn normalize_custom_target_root(value: &str) -> Result<String, &'static str> {
    validate_custom_target_root(value)?;
    let normalized = std::path::Path::new(value)
        .components()
        .filter_map(|component| match component {
            std::path::Component::CurDir => None,
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    Ok(if normalized.is_empty() {
        ".".into()
    } else {
        normalized
    })
}

pub(crate) fn validate_custom_artifact_paths(paths: &[String]) -> Result<(), &'static str> {
    if paths.is_empty() || paths.len() > 128 {
        return Err("custom targets require 1 to 128 build artifact paths");
    }
    let mut unique = std::collections::BTreeSet::new();
    for path in paths {
        if path.chars().any(char::is_control) {
            return Err("artifact paths cannot contain control characters");
        }
        validate_relative_path(path).map_err(|_| "artifact paths must be safe and relative")?;
        if path.contains('\\')
            || path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".." | ".git"))
            || !unique.insert(path)
        {
            return Err("artifact paths must be canonical, unique, and exclude Git metadata");
        }
    }
    Ok(())
}

fn valid_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && ![
            "HOME",
            "PATH",
            "SHELL",
            "DOCKER_HOST",
            "SSH_AUTH_SOCK",
            "LD_PRELOAD",
        ]
        .contains(&upper.as_str())
        && !["TOKEN", "SECRET", "PASSWORD", "PRIVATE_KEY", "CREDENTIAL"]
            .iter()
            .any(|marker| upper.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_custom_target_root, validate_custom_artifact_paths, validate_custom_target_root,
    };

    #[test]
    fn custom_target_root_accepts_repository_root_and_safe_relative_paths() {
        assert!(validate_custom_target_root(".").is_ok());
        assert!(validate_custom_target_root("./").is_ok());
        assert!(validate_custom_target_root("apps/site").is_ok());
        assert_eq!(normalize_custom_target_root("./").unwrap(), ".");
        assert_eq!(
            normalize_custom_target_root("./apps/site").unwrap(),
            "apps/site"
        );
    }

    #[test]
    fn custom_target_root_rejects_absolute_and_traversal_paths() {
        assert!(validate_custom_target_root("/workspace/app").is_err());
        assert!(validate_custom_target_root("../outside").is_err());
        assert!(validate_custom_target_root("apps/../../outside").is_err());
        assert!(validate_custom_target_root("apps\\\\site").is_err());
        assert!(validate_custom_target_root("apps/\nsite").is_err());
    }

    #[test]
    fn custom_artifact_paths_reject_curdir_and_duplicates() {
        assert!(validate_custom_artifact_paths(&["dist/index.html".into()]).is_ok());
        assert!(validate_custom_artifact_paths(&["./dist/index.html".into()]).is_err());
        assert!(
            validate_custom_artifact_paths(&["dist/index.html".into(), "dist/index.html".into()])
                .is_err()
        );
        assert!(validate_custom_artifact_paths(&["dist/\tindex.html".into()]).is_err());
    }
}
