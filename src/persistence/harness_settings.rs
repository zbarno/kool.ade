//! Operator-local discovery and default selection for coding harnesses.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const FILE: &str = "harness-settings.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HarnessSettings {
    pub schema_version: u8,
    pub default_harness: Option<String>,
    pub discovered: BTreeMap<String, DetectedHarness>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedHarness {
    pub status: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub diagnostic: Option<String>,
    pub ready: bool,
    #[serde(default)]
    pub configuration_required: bool,
}

pub fn settings_path() -> std::path::PathBuf {
    crate::persistence::state_root().join(FILE)
}

pub fn load() -> (HarnessSettings, Option<String>) {
    load_from(&settings_path())
}

fn load_from(path: &Path) -> (HarnessSettings, Option<String>) {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<HarnessSettings>(&bytes) {
            Ok(settings) if settings.schema_version == 1 => (settings, None),
            Ok(settings) => (
                HarnessSettings::default(),
                Some(format!(
                    "Unsupported harness settings version {} in {}",
                    settings.schema_version,
                    path.display()
                )),
            ),
            Err(error) => (
                HarnessSettings::default(),
                Some(format!("Could not read {}: {error}", path.display())),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (HarnessSettings::default(), None)
        }
        Err(error) => (
            HarnessSettings::default(),
            Some(format!("Could not read {}: {error}", path.display())),
        ),
    }
}

pub fn save(settings: &HarnessSettings) -> anyhow::Result<()> {
    save_to(&settings_path(), settings)
}

fn save_to(path: &Path, settings: &HarnessSettings) -> anyhow::Result<()> {
    let mut next = settings.clone();
    next.schema_version = 1;
    let json = serde_json::to_string_pretty(&next)?;
    crate::artifacts::atomic_write(path, &json)
}

#[cfg(test)]
#[path = "harness_settings/tests.rs"]
mod tests;
