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
        if self.source == RecipeSource::UserApproved
            && !self
                .evidence_refs
                .iter()
                .any(|reference| reference.starts_with("user-approval:"))
        {
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

pub(super) fn validate_relative_path(value: &str) -> Result<(), String> {
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
