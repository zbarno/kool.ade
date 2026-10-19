//! Operator-local discovery and default selection for coding harnesses.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const FILE: &str = "harness-settings.json";
pub const DISCOVERY_FRESHNESS_MS: u64 = 15 * 60 * 1000;
const CURRENT_SCHEMA_VERSION: u8 = 4;

#[cfg(test)]
thread_local! {
    static SETTINGS_PATH_OVERRIDE: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct RestoreSettingsPath(Option<std::path::PathBuf>);

#[cfg(test)]
impl Drop for RestoreSettingsPath {
    fn drop(&mut self) {
        SETTINGS_PATH_OVERRIDE.with(|path| *path.borrow_mut() = self.0.take());
    }
}

#[cfg(test)]
pub(crate) fn with_settings_path<R>(path: std::path::PathBuf, run: impl FnOnce() -> R) -> R {
    let previous = SETTINGS_PATH_OVERRIDE.with(|slot| slot.replace(Some(path)));
    let _restore = RestoreSettingsPath(previous);
    run()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HarnessSettings {
    pub schema_version: u8,
    pub default_harness: Option<String>,
    /// Extensible category identifiers mapped to locally available harnesses.
    pub work_routes: BTreeMap<String, WorkRoute>,
    pub discovered: BTreeMap<String, DetectedHarness>,
    /// Operator-selected executable paths keyed by supported harness id.
    /// These are consulted by both discovery and task execution.
    pub manual_executable_paths: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkRoute {
    pub harness: String,
    /// CLI-native model identifier. Empty means use that harness's configured default.
    pub model: Option<String>,
}

pub const IMPLEMENTATION: &str = "implementation";
pub const MANAGER: &str = "manager";
pub const PLAN_REVIEW: &str = "plan_review";
pub const QA: &str = "qa_verification";
pub const DOCUMENTATION: &str = "documentation";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedHarness {
    pub status: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub diagnostic: Option<String>,
    pub ready: bool,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub model_catalog_fresh: bool,
    #[serde(default)]
    pub configuration_required: bool,
    #[serde(default)]
    pub implementation_available: bool,
    /// Detailed per-mode and per-role evidence. Older settings omit this field;
    /// consumers requiring mode evidence must treat that as unknown.
    #[serde(default)]
    pub capability_snapshot: Option<crate::execution::contract::HarnessCapabilitySnapshotV1>,
    /// Unix time when this provider result and model catalog were last refreshed.
    #[serde(default)]
    pub checked_at_unix_ms: u64,
}

impl DetectedHarness {
    pub fn has_current_authenticated_capability(
        &self,
        provider_id: &str,
        now_unix_ms: u64,
    ) -> bool {
        use crate::execution::contract::ReadinessState;

        if !self.is_fresh_at(now_unix_ms) || !self.ready {
            return false;
        }
        self.capability_snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.provider_id == provider_id
                && snapshot.validate().is_ok()
                && snapshot.cli_binary == ReadinessState::Ready
                && snapshot.authentication == ReadinessState::Ready
        })
    }

    pub fn model_catalog_status(&self) -> crate::harness::discovery::ModelCatalogStatus {
        use crate::harness::discovery::ModelCatalogStatus;

        if self.model_catalog_fresh {
            ModelCatalogStatus::Fresh
        } else if !self.models.is_empty() {
            ModelCatalogStatus::Stale
        } else if self.capability_snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.model_catalog == crate::execution::contract::ReadinessState::Unavailable
        }) {
            ModelCatalogStatus::Unavailable
        } else {
            ModelCatalogStatus::NotChecked
        }
    }

    pub fn is_fresh_at(&self, now_unix_ms: u64) -> bool {
        self.checked_at_unix_ms > 0
            && self.checked_at_unix_ms <= now_unix_ms
            && now_unix_ms.saturating_sub(self.checked_at_unix_ms) <= DISCOVERY_FRESHNESS_MS
    }

    pub fn supports_role_at(
        &self,
        role: crate::execution::contract::HarnessRole,
        now_unix_ms: u64,
    ) -> bool {
        self.is_fresh_at(now_unix_ms)
            && self
                .capability_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.supports_role(role))
    }
}

pub fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub fn settings_path() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(path) = SETTINGS_PATH_OVERRIDE.with(|path| path.borrow().clone()) {
        return path;
    }
    crate::persistence::state_root().join(FILE)
}

pub fn load() -> (HarnessSettings, Option<String>) {
    load_from(&settings_path())
}

fn load_from(path: &Path) -> (HarnessSettings, Option<String>) {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<HarnessSettings>(&bytes) {
            Ok(mut settings) if (1..=CURRENT_SCHEMA_VERSION).contains(&settings.schema_version) => {
                if settings.schema_version < CURRENT_SCHEMA_VERSION {
                    // Before schema 4, successful executable probes were also
                    // persisted as positive authentication evidence. That
                    // inference is not valid; force a fresh provider auth probe.
                    for detected in settings.discovered.values_mut() {
                        if let Some(snapshot) = detected.capability_snapshot.as_mut() {
                            snapshot.authentication =
                                crate::execution::contract::ReadinessState::Unknown;
                        }
                    }
                }
                settings.schema_version = CURRENT_SCHEMA_VERSION;
                (settings, None)
            }
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
    next.schema_version = CURRENT_SCHEMA_VERSION;
    let json = serde_json::to_string_pretty(&next)?;
    crate::artifacts::atomic_write(path, &json)
}

#[cfg(test)]
#[path = "harness_settings/tests.rs"]
mod tests;
