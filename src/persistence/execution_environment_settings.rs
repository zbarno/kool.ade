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
}

impl Default for ExecutionEnvironmentSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            default_target: TargetPreference::Auto,
            task_targets: BTreeMap::new(),
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
            Ok(settings) if settings.schema_version == 1 => (settings, None),
            Ok(settings) => (
                ExecutionEnvironmentSettings::default(),
                Some(format!(
                    "Unsupported execution environment settings version {}",
                    settings.schema_version
                )),
            ),
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
