//! Per-device opt-in for capabilities that are implemented but not release enabled.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const FILE: &str = "experimental-features.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentalFeature {
    BrowserQa,
    LinuxDesktopQa,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExperimentalFeatureSettingsV1 {
    pub schema_version: u8,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub confirmed_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub enabled_features: BTreeSet<ExperimentalFeature>,
}

impl Default for ExperimentalFeatureSettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: false,
            confirmed_at_unix_ms: None,
            enabled_features: BTreeSet::new(),
        }
    }
}

impl ExperimentalFeatureSettingsV1 {
    pub fn feature_enabled(&self, feature: ExperimentalFeature) -> bool {
        self.enabled && self.enabled_features.contains(&feature)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported experimental settings version"
        );
        anyhow::ensure!(
            !self.enabled || self.confirmed_at_unix_ms.is_some_and(|time| time > 0),
            "experimental mode requires explicit local confirmation"
        );
        anyhow::ensure!(
            self.confirmed_at_unix_ms.is_none_or(|time| time > 0),
            "experimental confirmation timestamp is invalid"
        );
        Ok(())
    }
}

pub fn settings_path() -> std::path::PathBuf {
    crate::persistence::state_root().join(FILE)
}

pub fn load() -> (ExperimentalFeatureSettingsV1, Option<String>) {
    load_from(&settings_path())
}

fn load_from(path: &std::path::Path) -> (ExperimentalFeatureSettingsV1, Option<String>) {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<ExperimentalFeatureSettingsV1>(&bytes) {
            Ok(settings) => match settings.validate() {
                Ok(()) => (settings, None),
                Err(error) => (
                    ExperimentalFeatureSettingsV1::default(),
                    Some(format!("Invalid experimental feature settings: {error}")),
                ),
            },
            Err(error) => (
                ExperimentalFeatureSettingsV1::default(),
                Some(format!(
                    "Could not read experimental feature settings: {error}"
                )),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (ExperimentalFeatureSettingsV1::default(), None)
        }
        Err(error) => (
            ExperimentalFeatureSettingsV1::default(),
            Some(format!(
                "Could not read experimental feature settings: {error}"
            )),
        ),
    }
}

pub fn save(settings: &ExperimentalFeatureSettingsV1) -> anyhow::Result<()> {
    save_to(&settings_path(), settings)
}

fn save_to(path: &std::path::Path, settings: &ExperimentalFeatureSettingsV1) -> anyhow::Result<()> {
    settings.validate()?;
    crate::artifacts::atomic_write(path, &serde_json::to_string_pretty(settings)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "experimental_features/tests.rs"]
mod tests;
