//! Operator-local execution target defaults.

use crate::execution::contract::TargetPreference;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const FILE: &str = "execution-environments.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionEnvironmentSettings {
    pub schema_version: u8,
    pub default_target: TargetPreference,
    /// Task-specific choices stay in operator-local state, never in the project plan.
    #[serde(default)]
    pub task_targets: BTreeMap<String, TargetPreference>,
    /// Task capabilities are operator-local constraints and never enter the
    /// planning repository. Unsupported capabilities fail closed at routing.
    #[serde(default)]
    pub task_capabilities: BTreeMap<String, TaskCapabilityRequirements>,
    /// Optional per-task allowlist for targets selected by Auto or explicitly.
    /// An empty or absent list leaves the enrolled target pool unrestricted.
    #[serde(default)]
    pub task_allowed_targets: BTreeMap<String, Vec<TargetPreference>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TaskCapabilityRequirements {
    #[serde(default)]
    pub requires_gpu: bool,
    #[serde(default)]
    pub requires_display: bool,
}

impl Default for ExecutionEnvironmentSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            default_target: TargetPreference::Auto,
            task_targets: BTreeMap::new(),
            task_capabilities: BTreeMap::new(),
            task_allowed_targets: BTreeMap::new(),
        }
    }
}

pub fn settings_path() -> std::path::PathBuf {
    crate::persistence::state_root().join(FILE)
}

pub fn load() -> (ExecutionEnvironmentSettings, Option<String>) {
    load_from(&settings_path())
}

fn load_from(path: &Path) -> (ExecutionEnvironmentSettings, Option<String>) {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<ExecutionEnvironmentSettings>(&bytes) {
            Ok(settings) => match validate_settings(&settings) {
                Ok(()) => (settings, None),
                Err(error) => (
                    ExecutionEnvironmentSettings::default(),
                    Some(format!("Invalid execution environment settings: {error}")),
                ),
            },
            Err(error) => (
                ExecutionEnvironmentSettings::default(),
                Some(format!(
                    "Could not read execution environment settings: {error}"
                )),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (ExecutionEnvironmentSettings::default(), None)
        }
        Err(error) => (
            ExecutionEnvironmentSettings::default(),
            Some(format!(
                "Could not read execution environment settings: {error}"
            )),
        ),
    }
}

pub fn save(settings: &ExecutionEnvironmentSettings) -> anyhow::Result<()> {
    save_to(&settings_path(), settings)
}

fn save_to(path: &Path, settings: &ExecutionEnvironmentSettings) -> anyhow::Result<()> {
    validate_settings(settings)?;
    let mut settings = settings.clone();
    settings.schema_version = 1;
    crate::artifacts::atomic_write(path, &serde_json::to_string_pretty(&settings)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn validate_settings(settings: &ExecutionEnvironmentSettings) -> anyhow::Result<()> {
    anyhow::ensure!(settings.schema_version == 1, "unsupported settings version");
    if let TargetPreference::Worker(worker_id) = &settings.default_target {
        anyhow::ensure!(valid_worker_id(worker_id), "worker ID is invalid");
    }
    for (task_key, preference) in &settings.task_targets {
        anyhow::ensure!(valid_task_key(task_key), "task key is invalid");
        if let TargetPreference::Worker(worker_id) = preference {
            anyhow::ensure!(valid_worker_id(worker_id), "worker ID is invalid");
        }
    }
    for task_key in settings.task_capabilities.keys() {
        anyhow::ensure!(valid_task_key(task_key), "task key is invalid");
    }
    for (task_key, targets) in &settings.task_allowed_targets {
        anyhow::ensure!(valid_task_key(task_key), "task key is invalid");
        anyhow::ensure!(
            targets.iter().all(|target| match target {
                TargetPreference::Local => true,
                TargetPreference::Worker(worker_id) => valid_worker_id(worker_id),
                TargetPreference::Auto => false,
            }),
            "task target allowlist contains an invalid target"
        );
        for (index, target) in targets.iter().enumerate() {
            anyhow::ensure!(
                !targets[..index].contains(target),
                "task target allowlist contains duplicates"
            );
        }
    }
    Ok(())
}

fn update_task_allowed_targets(
    settings: &mut ExecutionEnvironmentSettings,
    task_key: &str,
    targets: Vec<TargetPreference>,
) {
    if targets.is_empty() {
        settings.task_allowed_targets.remove(task_key);
    } else {
        settings
            .task_allowed_targets
            .insert(task_key.to_owned(), targets);
    }
}

pub fn task_target(task_key: &str) -> Option<TargetPreference> {
    let (settings, _) = load();
    settings.task_targets.get(task_key).cloned()
}

pub fn save_task_target(task_key: &str, preference: TargetPreference) -> anyhow::Result<()> {
    anyhow::ensure!(valid_task_key(task_key), "task key is invalid");
    let (mut settings, error) = load();
    anyhow::ensure!(error.is_none(), "cannot update invalid execution settings");
    if let TargetPreference::Worker(worker_id) = &preference {
        anyhow::ensure!(valid_worker_id(worker_id), "worker ID is invalid");
        let workers = crate::persistence::remote_workers::list()?;
        let worker = workers
            .iter()
            .find(|worker| worker.worker_id == *worker_id)
            .ok_or_else(|| anyhow::anyhow!("worker is not enrolled"))?;
        anyhow::ensure!(worker.enabled, "worker is disabled");
        anyhow::ensure!(
            worker.last_probe.as_ref().is_some_and(|probe| {
                probe.health == crate::persistence::remote_workers::RemoteWorkerHealth::Healthy
            }),
            "worker is not healthy"
        );
    }
    settings
        .task_targets
        .insert(task_key.to_owned(), preference);
    save(&settings)
}

pub fn task_capabilities(task_key: Option<&str>) -> TaskCapabilityRequirements {
    let Some(task_key) = task_key else {
        return TaskCapabilityRequirements::default();
    };
    load()
        .0
        .task_capabilities
        .get(task_key)
        .copied()
        .unwrap_or_default()
}

pub fn save_task_capabilities(
    task_key: &str,
    requirements: TaskCapabilityRequirements,
) -> anyhow::Result<()> {
    anyhow::ensure!(valid_task_key(task_key), "task key is invalid");
    let (mut settings, error) = load();
    anyhow::ensure!(error.is_none(), "cannot update invalid execution settings");
    if requirements == TaskCapabilityRequirements::default() {
        settings.task_capabilities.remove(task_key);
    } else {
        settings
            .task_capabilities
            .insert(task_key.to_owned(), requirements);
    }
    save(&settings)
}

pub fn save_task_allowed_targets(
    task_key: &str,
    targets: Vec<TargetPreference>,
) -> anyhow::Result<()> {
    anyhow::ensure!(valid_task_key(task_key), "task key is invalid");
    let (mut settings, error) = load();
    anyhow::ensure!(error.is_none(), "cannot update invalid execution settings");
    update_task_allowed_targets(&mut settings, task_key, targets);
    save(&settings)
}

fn valid_task_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn valid_worker_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
#[path = "execution_environment_settings/tests.rs"]
mod tests;
